//! Rename state and the operations its panels can perform.
pub(crate) mod screen;
use crate::action::{Action, Effect};
use crate::model::{Cursor, FilePreview, Panels};
use crate::modes::context::ModeContext;
use crate::modes::review::ReviewState;
use std::collections::BTreeSet;
use std::path::Path;
use vvv_engine::protocol::FileChange;
use vvv_engine::{Confidence, Match, MatchId, Occurrence, RelPath, SymbolKind};
use vvv_engine::{Intent, RenameIntent, Selection};

/// What `r` found under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTarget {
    pub name: String,
    pub symbol: Option<SymbolKind>,
    pub declared_in: Option<RelPath>,
}

// ---------------------------------------------------------------- rename

/// A rename being judged: the new name, every occurrence by verdict, and
/// which of them are ticked for the commit.
#[derive(Debug)]
pub struct RenameMode {
    pub target: RenameTarget,
    pub language: Option<vvv_engine::LanguageId>,
    /// The new name, edited live; the plan is re-made as it grows.
    pub name: String,
    pub caret: crate::input::Caret,
    pub declarations: Vec<Match>,
    pub occurrences: Vec<Occurrence>,
    /// The last plan's files, each holding its diff: the preview the detail
    /// pane draws.
    pub changes: Vec<FileChange>,
    pub ticks: BTreeSet<MatchId>,
    pub focus: RenamePanel,
    /// Cursors of the `?`, `✓` and `✗` panels, in that order.
    pub cursors: [Cursor; 3],
    /// The list panel the detail follows when focus is elsewhere.
    pub last_list: RenamePanel,
    pub detail_scroll: usize,
    pub preview: Option<FilePreview>,
    /// The verdicts are in and the ticks seeded; later plans only refresh
    /// `changes`, so typing does not undo the user's ticks.
    pub judged: bool,
    /// Waiting for the judge, or for the commit.
    pub busy: bool,
    pub applying: bool,
    pub error: Option<crate::problem::Problem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenamePanel {
    Name,
    Unsure,
    Sure,
    Other,
    Detail,
}

impl Panels for RenamePanel {
    const ALL: &'static [Self] = &[
        Self::Name,
        Self::Unsure,
        Self::Sure,
        Self::Other,
        Self::Detail,
    ];
}

impl RenamePanel {
    pub fn confidence(self) -> Option<Confidence> {
        match self {
            Self::Unsure => Some(Confidence::Unresolved),
            Self::Sure => Some(Confidence::Resolved),
            Self::Other => Some(Confidence::Other),
            Self::Name | Self::Detail => None,
        }
    }

    fn slot(self) -> Option<usize> {
        match self {
            Self::Unsure => Some(0),
            Self::Sure => Some(1),
            Self::Other => Some(2),
            Self::Name | Self::Detail => None,
        }
    }
}

impl RenameMode {
    pub fn state(&self) -> ReviewState<'_> {
        if self.applying {
            ReviewState::Applying
        } else if self.busy {
            ReviewState::Planning
        } else if let Some(error) = &self.error {
            ReviewState::Failed(error.message())
        } else if self.name.trim().is_empty() || self.name.trim() == self.target.name {
            ReviewState::Input("type a new name")
        } else if self.ticks.is_empty() {
            ReviewState::Empty("select sites")
        } else if self.changes.is_empty() {
            ReviewState::Empty("no changes")
        } else {
            ReviewState::Ready
        }
    }

    pub fn new(target: RenameTarget, language: Option<vvv_engine::LanguageId>) -> Self {
        Self {
            name: String::new(),
            caret: Default::default(),
            target,
            language,
            declarations: Vec::new(),
            occurrences: Vec::new(),
            changes: Vec::new(),
            ticks: BTreeSet::new(),
            focus: RenamePanel::Name,
            cursors: [Cursor::default(); 3],
            last_list: RenamePanel::Unsure,
            detail_scroll: 0,
            preview: None,
            judged: false,
            busy: true,
            applying: false,
            error: None,
        }
    }

    /// The rows of one verdict panel, in the engine's order.
    pub fn rows(&self, confidence: Confidence) -> Vec<&Occurrence> {
        self.occurrences
            .iter()
            .filter(|o| o.confidence == confidence)
            .collect()
    }

    pub fn cursor(&self, panel: RenamePanel) -> Option<&Cursor> {
        panel.slot().map(|i| &self.cursors[i])
    }

    pub fn cursor_mut(&mut self, panel: RenamePanel) -> Option<&mut Cursor> {
        panel.slot().map(move |i| &mut self.cursors[i])
    }

    /// The list panel whose row the detail explains.
    pub fn list(&self) -> RenamePanel {
        if self.focus.confidence().is_some() {
            self.focus
        } else {
            self.last_list
        }
    }

    pub fn current(&self) -> Option<&Occurrence> {
        let panel = self.list();
        let rows = self.rows(panel.confidence()?);
        rows.get(self.cursor(panel)?.index).copied()
    }

    pub fn is_ticked(&self, o: &Occurrence) -> bool {
        self.ticks.contains(&o.m.id)
    }

    pub fn ticked(&self, confidence: Confidence) -> usize {
        self.rows(confidence)
            .iter()
            .filter(|o| self.is_ticked(o))
            .count()
    }

    pub fn toggle(&mut self) {
        if let Some(id) = self.current().map(|o| o.m.id.clone())
            && !self.ticks.remove(&id)
        {
            self.ticks.insert(id);
        }
    }

    /// Tick every row of the focused panel, or untick them all when they
    /// already are.
    pub fn toggle_panel(&mut self) {
        let Some(confidence) = self.list().confidence() else {
            return;
        };
        let ids: Vec<MatchId> = self
            .rows(confidence)
            .iter()
            .map(|o| o.m.id.clone())
            .collect();
        if ids.iter().all(|id| self.ticks.contains(id)) {
            for id in &ids {
                self.ticks.remove(id);
            }
        } else {
            self.ticks.extend(ids);
        }
    }

    /// The plan's change for the occurrence's file when the plan edits this
    /// very site: the diff the detail pane draws. A file with only other
    /// sites changed is not this row's preview.
    pub fn file(&self, o: &Occurrence) -> Option<&FileChange> {
        self.changes
            .iter()
            .find(|f| f.path == o.m.path && f.edits.iter().any(|e| e.span == o.m.span))
    }

    /// Files the commit touches, from the ticks.
    pub fn files(&self) -> usize {
        self.occurrences
            .iter()
            .filter(|o| self.is_ticked(o))
            .map(|o| o.m.path.as_path())
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
                return self.plan(generation, true);
            }
            Action::Backspace => {
                context.status.clear();
                self.input(None);
                let generation = context.next_generation();
                return self.plan(generation, true);
            }
            Action::Clear => {
                context.status.clear();
                self.name.clear();
                let generation = context.next_generation();
                return self.plan(generation, true);
            }
            Action::Enter => return self.commit(context),
            Action::Toggle => return self.toggled(false, context),
            Action::ToggleAll => return self.toggled(true, context),
            _ => return Vec::new(),
        }
        self.preview_effect()
    }
    pub fn edit_input(
        &mut self,
        edit: crate::input::Edit<'_>,
        context: &mut ModeContext<'_>,
    ) -> Vec<Effect> {
        if self.applying {
            return Vec::new();
        }
        if !crate::input::TextInput::new(&mut self.name, &mut self.caret).apply(edit) {
            return Vec::new();
        }
        context.status.clear();
        let generation = context.next_generation();
        self.plan(generation, true)
    }
    pub fn input(&mut self, c: Option<char>) {
        crate::input::TextInput::new(&mut self.name, &mut self.caret).edit(c);
    }
    pub fn focus_by(&mut self, by: i32) {
        self.focus = self.focus.step(by);
        while self
            .focus
            .confidence()
            .is_some_and(|c| self.rows(c).is_empty())
        {
            self.focus = self.focus.step(by);
        }
        self.remember_list();
    }
    pub fn focus_nth(&mut self, n: u8) {
        if let Some(p) = RenamePanel::nth(n) {
            self.focus = p;
            self.remember_list();
        };
    }
    fn remember_list(&mut self) {
        if self.focus.confidence().is_some() {
            self.last_list = self.focus;
        }
    }
    pub fn moved(&mut self, by: i32) {
        let panel = self.list();
        if let Some(confidence) = panel.confidence() {
            let len = self.rows(confidence).len();
            if let Some(cursor) = self.cursor_mut(panel) {
                cursor.move_by(by, len);
            }
            self.detail_scroll = 0;
        };
    }
    pub fn scrolled(&mut self, by: i32) {
        self.detail_scroll = self.detail_scroll.saturating_add_signed(by as isize);
    }
    pub fn plan(&mut self, generation: u64, debounce: bool) -> Vec<Effect> {
        if self.judged && (self.name.trim().is_empty() || self.ticks.is_empty()) {
            self.changes.clear();
            self.error = None;
            self.busy = false;
            self.applying = false;
            return Vec::new();
        }
        let name = if self.name.trim().is_empty() {
            self.target.name.as_str()
        } else {
            self.name.trim()
        };
        // Until the first judgment arrives, the engine must choose the defaults.
        let mut intent = RenameIntent::new(&self.target.name, name);
        if self.judged {
            intent.selection = Selection::Ids(self.ticks.clone());
        }
        intent.references.symbol = self.target.symbol;
        intent.references.language = self.language.clone();
        intent.references.declared_in = self.target.declared_in.clone();
        self.busy = true;
        vec![Effect::Plan {
            generation,
            intent: Intent::Rename(intent),
            debounce,
        }]
    }
    pub fn commit(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        if self.busy || self.occurrences.is_empty() {
            return Vec::new();
        }
        if let Some(error) = &self.error {
            return context.fail(error.message());
        }
        let to = self.name.trim().to_owned();
        if to.is_empty() || to == self.target.name {
            return context.fail("type the new name first");
        }
        if self.ticks.is_empty() {
            return context.fail("nothing ticked");
        }
        if self.changes.is_empty() {
            return context.fail("no changes to apply");
        }
        let mut intent =
            RenameIntent::new(&self.target.name, to).selecting(Selection::Ids(self.ticks.clone()));
        intent.references.symbol = self.target.symbol;
        intent.references.language = self.language.clone();
        intent.references.declared_in = self.target.declared_in.clone();
        self.busy = true;
        self.applying = true;
        context.status.busy = true;
        vec![Effect::Commit {
            generation: *context.generation,
            intent: Intent::Rename(intent),
        }]
    }
    pub fn planned(
        &mut self,
        declarations: Vec<Match>,
        occurrences: Vec<Occurrence>,
        files: Vec<FileChange>,
    ) -> Vec<Effect> {
        self.declarations = declarations;
        self.occurrences = occurrences;
        self.changes = files;
        self.busy = false;
        self.applying = false;
        self.error = None;
        // The first plan seeds the ticks from the engine's default:
        // what its plan with no selection edits. Later plans (typing
        // the new name) only refresh the diff, so the ticks stand.
        if !self.judged {
            self.ticks = self
                .occurrences
                .iter()
                .filter(|o| {
                    self.changes
                        .iter()
                        .any(|f| f.path == o.m.path && f.edits.iter().any(|e| e.span == o.m.span))
                })
                .map(|o| o.m.id.clone())
                .collect();
            self.judged = true;
            for cursor in &mut self.cursors {
                cursor.index = 0;
            }
            // Start where judgment is needed; `✓` when there is nothing to judge.
            self.last_list = if self.rows(Confidence::Unresolved).is_empty() {
                RenamePanel::Sure
            } else {
                RenamePanel::Unsure
            };
        }
        self.preview_effect()
    }
    pub fn toggled(&mut self, all: bool, context: &mut ModeContext<'_>) -> Vec<Effect> {
        if all {
            self.toggle_panel();
        } else {
            self.toggle();
            self.moved(1);
        }
        let generation = context.next_generation();
        let mut effects = self.preview_effect();
        effects.extend(self.plan(generation, true));
        effects
    }
    pub fn from_results(results: &crate::modes::search::Results) -> Result<Self, &'static str> {
        let (target, language) = match &results.subject {
            Some(s) => (
                RenameTarget {
                    name: s.name.clone(),
                    symbol: s.symbol,
                    declared_in: s.declared_in.clone(),
                },
                s.language.clone(),
            ),
            None => {
                let Some(target) = results.rename_target() else {
                    return Err("put the cursor on an identifier or a declaration to rename");
                };
                let language = results.query.as_ref().and_then(|q| q.language().cloned());
                (target, language)
            }
        };

        Ok(Self::new(target, language))
    }
    pub fn preview_effect(&self) -> Vec<Effect> {
        match self.current().map(|o| o.m.path.clone()) {
            Some(path) if self.preview.as_ref().map(|p| &p.path) != Some(&path) => {
                vec![Effect::Preview { path }]
            }
            _ => Vec::new(),
        }
    }
    pub fn site(&self) -> Option<(RelPath, u32)> {
        self.current().map(|o| (o.m.path.clone(), o.m.start.line))
    }
    pub fn scroll_focused(&self) -> bool {
        self.focus == RenamePanel::Detail
    }
    pub fn previewed(&mut self, preview: FilePreview) {
        if self.current().is_none_or(|row| row.m.path != preview.path) {
            return;
        }
        if self.preview.as_ref().is_none_or(|p| p.path != preview.path) {
            self.detail_scroll = 0;
        }
        self.preview = Some(preview);
    }
    pub fn plan_failed(&mut self, problem: crate::problem::Problem) {
        self.busy = false;
        self.applying = false;
        self.error = Some(problem);
    }
    pub fn judgment(&self) -> RenameIntent {
        let mut intent = RenameIntent::new(&self.target.name, &self.target.name);
        intent.references.symbol = self.target.symbol;
        intent.references.language = self.language.clone();
        intent.references.declared_in = self.target.declared_in.clone();
        intent
    }
}
