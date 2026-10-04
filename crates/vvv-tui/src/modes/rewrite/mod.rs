//! Rewrite state and ticked matches.
pub(crate) mod screen;
use crate::action::{Action, Effect};
use crate::model::{Cursor, Panels};
use crate::modes::context::ModeContext;
use crate::modes::review::ReviewState;
use std::collections::BTreeSet;
use std::path::Path;
use vvv_engine::protocol::FileChange;
use vvv_engine::{Intent, RelPath, Selection};
use vvv_engine::{Match, MatchId, Query};
// ---------------------------------------------------------------- rewrite

/// A rewrite being shaped: the search's matches, the template edited live,
/// and what each match becomes.
#[derive(Debug)]
pub struct RewriteMode {
    pub query: Query,
    pub template: String,
    pub caret: crate::input::Caret,
    pub matches: Vec<Match>,
    /// The last plan's files, each holding its diff: the preview the detail
    /// pane draws.
    pub changes: Vec<FileChange>,
    pub ticks: BTreeSet<MatchId>,
    pub focus: RewritePanel,
    pub cursor: Cursor,
    pub detail_scroll: usize,
    pub error: Option<crate::problem::Problem>,
    pub busy: bool,
    pub applying: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewritePanel {
    Template,
    Matches,
    Detail,
}

impl Panels for RewritePanel {
    const ALL: &'static [Self] = &[Self::Template, Self::Matches, Self::Detail];
}

impl RewriteMode {
    pub fn state(&self) -> ReviewState<'_> {
        if self.applying {
            ReviewState::Applying
        } else if self.busy {
            ReviewState::Planning
        } else if let Some(error) = &self.error {
            ReviewState::Failed(error.message())
        } else if self.template.trim().is_empty() {
            ReviewState::Input("type a template")
        } else if self.ticks.is_empty() {
            ReviewState::Empty("select matches")
        } else if self.changes.is_empty() {
            ReviewState::Empty("no changes")
        } else {
            ReviewState::Ready
        }
    }

    pub fn new(query: Query, matches: Vec<Match>) -> Self {
        let ticks = matches.iter().map(|m| m.id.clone()).collect();
        Self {
            query,
            template: String::new(),
            caret: Default::default(),
            matches,
            changes: Vec::new(),
            ticks,
            focus: RewritePanel::Template,
            cursor: Cursor::default(),
            detail_scroll: 0,
            error: None,
            busy: false,
            applying: false,
        }
    }

    pub fn intent(&self) -> Option<vvv_engine::RewriteIntent> {
        (!self.template.trim().is_empty())
            .then(|| vvv_engine::RewriteIntent::new(self.query.clone(), self.template.as_str()))
    }

    pub fn current(&self) -> Option<&Match> {
        self.matches.get(self.cursor.index)
    }

    pub fn is_ticked(&self, m: &Match) -> bool {
        self.ticks.contains(&m.id)
    }

    pub fn toggle(&mut self) {
        if let Some(id) = self.current().map(|m| m.id.clone())
            && !self.ticks.remove(&id)
        {
            self.ticks.insert(id);
        }
    }

    pub fn toggle_all(&mut self) {
        if self.ticks.len() == self.matches.len() {
            self.ticks.clear();
        } else {
            self.ticks = self.matches.iter().map(|m| m.id.clone()).collect();
        }
    }

    pub fn files(&self) -> usize {
        self.matches
            .iter()
            .filter(|m| self.is_ticked(m))
            .map(|m| m.path.as_path())
            .collect::<BTreeSet<&Path>>()
            .len()
    }

    pub fn update(&mut self, action: Action, context: &mut ModeContext<'_>) -> Vec<Effect> {
        if self.applying
            && matches!(
                action,
                Action::Input(_)
                    | Action::Backspace
                    | Action::Clear
                    | Action::Toggle
                    | Action::ToggleAll
            )
        {
            return Vec::new();
        }
        match action {
            Action::FocusNext => self.focus_by(1),
            Action::FocusPrev => self.focus_by(-1),
            Action::FocusNth(n) => self.focus_nth(n),
            Action::Move(n) => self.moved(n),
            Action::Top if self.scroll_focused() => {
                self.detail_scroll = 0;
                return Vec::new();
            }
            Action::Top => self.moved(i32::MIN / 2),
            Action::Bottom if self.scroll_focused() => {
                self.detail_scroll = usize::MAX;
                return Vec::new();
            }
            Action::Bottom => self.moved(i32::MAX / 2),
            Action::Scroll(n) if self.scroll_focused() => {
                self.scrolled(n);
                return Vec::new();
            }
            Action::Scroll(n) => self.moved(n),
            Action::Input(c) => {
                context.status.clear();
                self.input(Some(c));
                let generation = context.next_generation();
                return self.plan(generation);
            }
            Action::Backspace => {
                context.status.clear();
                self.input(None);
                let generation = context.next_generation();
                return self.plan(generation);
            }
            Action::Clear => {
                context.status.clear();
                self.template.clear();
                let generation = context.next_generation();
                return self.plan(generation);
            }
            Action::Enter => return self.commit(context),
            Action::Toggle => return self.toggled(false, context),
            Action::ToggleAll => return self.toggled(true, context),
            _ => return Vec::new(),
        }
        Vec::new()
    }
    pub fn focus_by(&mut self, by: i32) {
        self.focus = self.focus.step(by);
    }
    pub fn focus_nth(&mut self, n: u8) {
        if let Some(p) = RewritePanel::nth(n) {
            self.focus = p;
        }
    }
    pub fn moved(&mut self, by: i32) {
        let len = self.matches.len();
        self.cursor.move_by(by, len);
        self.detail_scroll = 0;
    }
    pub fn scrolled(&mut self, by: i32) {
        self.detail_scroll = self.detail_scroll.saturating_add_signed(by as isize);
    }
    pub fn plan(&mut self, generation: u64) -> Vec<Effect> {
        if self.ticks.is_empty() {
            self.changes.clear();
            self.error = None;
            self.busy = false;
            return Vec::new();
        }
        match self.intent() {
            Some(intent) => {
                self.busy = true;
                vec![Effect::Plan {
                    generation,
                    intent: Intent::Rewrite(intent.selecting(Selection::Ids(self.ticks.clone()))),
                    debounce: true,
                }]
            }
            None => {
                self.changes.clear();
                self.error = None;
                self.busy = false;
                self.applying = false;
                Vec::new()
            }
        }
    }
    pub fn commit(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        if self.busy {
            return Vec::new();
        }
        if let Some(error) = &self.error {
            return context.fail(error.message());
        }
        if self.changes.is_empty() {
            return context.fail("type a template first");
        }
        if self.ticks.is_empty() {
            return context.fail("nothing ticked");
        }
        let Some(intent) = self.intent() else {
            return Vec::new();
        };
        self.busy = true;
        self.applying = true;
        context.status.busy = true;
        vec![Effect::Commit {
            intent: Intent::Rewrite(intent.selecting(Selection::Ids(self.ticks.clone()))),
        }]
    }
    pub fn planned(&mut self, files: Vec<FileChange>) -> Vec<Effect> {
        self.changes = files;
        self.error = None;
        self.busy = false;
        self.applying = false;
        Vec::new()
    }
    pub fn toggled(&mut self, all: bool, context: &mut ModeContext<'_>) -> Vec<Effect> {
        if all {
            self.toggle_all();
        } else {
            self.toggle();
            self.moved(1);
        }
        let generation = context.next_generation();
        self.plan(generation)
    }
    pub fn from_results(results: &crate::modes::search::Results) -> Result<Self, &'static str> {
        let Some(query) = results.query.clone() else {
            return Err("search for the pattern to rewrite first");
        };
        // Rewrite is query-scoped: its ticks must be a subset of what the
        // query searches for, not the (possibly wider) anchored occurrences.
        let matches: Vec<_> = if results.is_anchored() {
            results.matches.iter().collect()
        } else {
            // File filtering is navigation-only; it cannot narrow a plan.
            results.eligible()
        };
        if matches.is_empty() {
            return Err("nothing matched; a rewrite acts on the matches");
        }
        Ok(Self::new(query, matches.into_iter().cloned().collect()))
    }
    pub fn scroll_focused(&self) -> bool {
        self.focus == RewritePanel::Detail
    }
    pub fn site(&self) -> Option<(RelPath, u32)> {
        self.current().map(|m| (m.path.clone(), m.start.line))
    }
    pub fn plan_failed(&mut self, problem: crate::problem::Problem) {
        self.busy = false;
        self.applying = false;
        self.error = Some(problem);
    }
    pub fn edit_input(
        &mut self,
        edit: crate::input::Edit<'_>,
        context: &mut ModeContext<'_>,
    ) -> Vec<Effect> {
        if self.applying {
            return Vec::new();
        }
        if !crate::input::TextInput::new(&mut self.template, &mut self.caret).apply(edit) {
            return Vec::new();
        }
        context.status.clear();
        let generation = context.next_generation();
        self.plan(generation)
    }
    pub fn input(&mut self, c: Option<char>) {
        crate::input::TextInput::new(&mut self.template, &mut self.caret).edit(c);
    }
}
