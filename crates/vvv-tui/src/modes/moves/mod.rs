//! File and symbol move state.
pub(crate) mod screen;
use crate::action::{Action, Effect};
use crate::model::{Cursor, FilePreview, Panels};
use crate::modes::context::ModeContext;
use crate::modes::review::ReviewState;
use vvv_engine::SymbolKind;
use vvv_engine::protocol::FileChange;
use vvv_engine::{Intent, Notice, RelPath, Respelling};
// ---------------------------------------------------------------- move

/// A move being planned: the destination, edited live, and the plan it
/// yields — or why it yields none.
#[derive(Debug)]
pub struct MoveMode {
    pub from: RelPath,
    /// `Some` when one declaration moves rather than the file.
    pub symbol: Option<String>,
    pub to: String,
    pub plan: Option<MovePlan>,
    pub error: Option<String>,
    pub focus: MovePanel,
    /// Cursors of the `→`, `±` and `!` panels.
    pub cursors: [Cursor; 3],
    pub last_list: MovePanel,
    pub detail_scroll: usize,
    pub preview: Option<FilePreview>,
    /// Show the whole file diff in the detail panel.
    pub diff: bool,
    pub busy: bool,
    pub applying: bool,
}

#[derive(Debug, Clone)]
pub struct MovePlan {
    pub intent: Intent,
    pub files: Vec<FileChange>,
    pub respellings: Vec<Respelling>,
    pub notices: Vec<Notice>,
    /// Indices into `files` of the structural changes: moved, or holding an
    /// edit no respelling accounts for.
    pub structural: Vec<usize>,
}

impl MovePlan {
    pub fn new(
        intent: Intent,
        files: Vec<FileChange>,
        respellings: Vec<Respelling>,
        notices: Vec<Notice>,
    ) -> Self {
        let structural = files
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                f.moved_to.is_some()
                    || f.edits.is_empty()
                    || !f.edits.iter().all(|e| {
                        respellings
                            .iter()
                            .any(|r| r.path == f.path && r.span == e.span)
                    })
            })
            .map(|(i, _)| i)
            .collect();
        Self {
            intent,
            files,
            respellings,
            notices,
            structural,
        }
    }

    /// One line per structural file: where it goes, or its first changed line.
    pub fn structural_label(&self, i: usize) -> String {
        let file = &self.files[i];
        if let Some(to) = &file.moved_to {
            return format!("→ {to}");
        }
        file.diff
            .as_str()
            .lines()
            .find(|l| {
                (l.starts_with('-') || l.starts_with('+'))
                    && !l.starts_with("---")
                    && !l.starts_with("+++")
            })
            .map(|l| format!("{} {}", &l[..1], l[1..].trim()))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovePanel {
    To,
    Respellings,
    Structural,
    Notices,
    Detail,
}

impl Panels for MovePanel {
    const ALL: &'static [Self] = &[
        Self::To,
        Self::Respellings,
        Self::Structural,
        Self::Notices,
        Self::Detail,
    ];
}

impl MovePanel {
    fn slot(self) -> Option<usize> {
        match self {
            Self::Respellings => Some(0),
            Self::Structural => Some(1),
            Self::Notices => Some(2),
            Self::To | Self::Detail => None,
        }
    }
}

/// What a move row points at, for the detail panel and the editor.
#[derive(Debug, Clone)]
pub enum MoveRow<'a> {
    Respelling(&'a Respelling),
    Structural(&'a FileChange),
    Notice(&'a Notice),
}

impl MoveRow<'_> {
    pub fn path(&self) -> &RelPath {
        match self {
            Self::Respelling(r) => &r.path,
            Self::Structural(f) => &f.path,
            Self::Notice(n) => &n.path,
        }
    }

    pub fn line(&self) -> u32 {
        match self {
            Self::Respelling(r) => r.start.line,
            Self::Structural(_) => 0,
            Self::Notice(n) => n.start.line,
        }
    }
}

impl MoveMode {
    pub fn state(&self) -> ReviewState<'_> {
        if self.applying {
            ReviewState::Applying
        } else if self.busy {
            ReviewState::Planning
        } else if let Some(error) = &self.error {
            ReviewState::Failed(error)
        } else if self.to.trim().is_empty() {
            ReviewState::Input("type a destination")
        } else if self.plan.as_ref().is_none_or(|plan| plan.files.is_empty()) {
            ReviewState::Empty("no changes")
        } else {
            ReviewState::Ready
        }
    }

    pub fn new(from: RelPath, symbol: Option<String>) -> Self {
        let to = match &symbol {
            Some(_) => String::new(),
            None => from.as_str().to_owned(),
        };
        Self {
            from,
            symbol,
            to,
            plan: None,
            error: None,
            focus: MovePanel::To,
            cursors: [Cursor::default(); 3],
            last_list: MovePanel::Respellings,
            detail_scroll: 0,
            preview: None,
            diff: false,
            busy: false,
            applying: false,
        }
    }

    pub fn intent(&self) -> Option<Intent> {
        let to = self.to.trim();
        if to.is_empty() {
            return None;
        }
        Some(match &self.symbol {
            Some(name) => {
                Intent::MoveSymbol(vvv_engine::MoveSymbolIntent::new(name, &self.from, to))
            }
            None => Intent::Move(vvv_engine::MoveIntent::new(&self.from, to)),
        })
    }

    pub fn len(&self, panel: MovePanel) -> usize {
        let Some(plan) = &self.plan else {
            return 0;
        };
        match panel {
            MovePanel::Respellings => plan.respellings.len(),
            MovePanel::Structural => plan.structural.len(),
            MovePanel::Notices => plan.notices.len(),
            MovePanel::To | MovePanel::Detail => 0,
        }
    }

    pub fn cursor(&self, panel: MovePanel) -> Option<&Cursor> {
        panel.slot().map(|i| &self.cursors[i])
    }

    pub fn cursor_mut(&mut self, panel: MovePanel) -> Option<&mut Cursor> {
        panel.slot().map(move |i| &mut self.cursors[i])
    }

    pub fn list(&self) -> MovePanel {
        if self.focus.slot().is_some() {
            self.focus
        } else {
            self.last_list
        }
    }

    pub fn current(&self) -> Option<MoveRow<'_>> {
        let plan = self.plan.as_ref()?;
        let panel = self.list();
        let i = self.cursor(panel)?.index;
        Some(match panel {
            MovePanel::Respellings => MoveRow::Respelling(plan.respellings.get(i)?),
            MovePanel::Structural => MoveRow::Structural(&plan.files[*plan.structural.get(i)?]),
            MovePanel::Notices => MoveRow::Notice(plan.notices.get(i)?),
            MovePanel::To | MovePanel::Detail => return None,
        })
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
                self.to.clear();
                let generation = context.next_generation();
                return self.plan(generation, true);
            }
            Action::Enter => return self.commit(context),
            Action::Diff => {
                self.toggle_diff();
                return Vec::new();
            }
            _ => return Vec::new(),
        }
        self.preview_effect()
    }
    pub fn focus_by(&mut self, by: i32) {
        self.focus = self.focus.step(by);
        while self.focus.slot().is_some() && self.len(self.focus) == 0 {
            self.focus = self.focus.step(by);
        }
        self.remember_list();
    }
    pub fn focus_nth(&mut self, n: u8) {
        if let Some(p) = MovePanel::nth(n) {
            self.focus = p;
            self.remember_list();
        };
    }
    fn remember_list(&mut self) {
        if self.focus.slot().is_some() {
            self.last_list = self.focus;
        }
    }
    pub fn moved(&mut self, by: i32) {
        let panel = self.list();
        let len = self.len(panel);
        if let Some(cursor) = self.cursor_mut(panel) {
            cursor.move_by(by, len);
        }
        self.detail_scroll = 0;
    }
    pub fn scrolled(&mut self, by: i32) {
        self.detail_scroll = self.detail_scroll.saturating_add_signed(by as isize);
    }
    pub fn plan(&mut self, generation: u64, debounce: bool) -> Vec<Effect> {
        match self.intent() {
            Some(intent) => {
                self.busy = true;
                vec![Effect::Plan {
                    generation,
                    intent,
                    debounce,
                }]
            }
            None => {
                self.plan = None;
                self.error = None;
                self.busy = false;
                self.applying = false;
                Vec::new()
            }
        }
    }
    pub fn commit(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        match &self.plan {
            Some(plan) if self.state() == ReviewState::Ready => {
                self.busy = true;
                self.applying = true;
                context.status.busy = true;
                vec![Effect::Commit {
                    intent: plan.intent.clone(),
                }]
            }
            _ => Vec::new(),
        }
    }
    pub fn planned(
        &mut self,
        intent: Intent,
        respellings: Vec<Respelling>,
        notices: Vec<Notice>,
        files: Vec<FileChange>,
    ) -> Vec<Effect> {
        self.plan = Some(MovePlan::new(intent, files, respellings, notices));
        self.error = None;
        self.busy = false;
        self.applying = false;
        for panel in [
            MovePanel::Respellings,
            MovePanel::Structural,
            MovePanel::Notices,
        ] {
            let len = self.len(panel);
            if let Some(cursor) = self.cursor_mut(panel) {
                cursor.clamp(len);
            }
        }
        if self.len(self.last_list) == 0 {
            self.last_list = if self.len(MovePanel::Respellings) > 0 {
                MovePanel::Respellings
            } else {
                MovePanel::Structural
            };
        }
        self.preview_effect()
    }
    pub fn from_results(
        results: &crate::modes::search::Results,
        symbol: bool,
    ) -> Result<Self, &'static str> {
        let (from, name) = match &results.subject {
            Some(s) => {
                let Some(from) = s.declared_in.clone() else {
                    return Err("the declaration has no file to move");
                };
                let name = if symbol {
                    match s.symbol.filter(|k| *k != SymbolKind::Impl) {
                        Some(_) => Some(s.name.clone()),
                        None => return Err("put the cursor on a declaration to move it"),
                    }
                } else {
                    None
                };
                (from, name)
            }
            None => {
                let Some(m) = results.current() else {
                    return Err("put the cursor on a match in the file to move");
                };
                let name = if symbol {
                    match m.symbol.as_ref().filter(|s| s.kind != SymbolKind::Impl) {
                        Some(s) => Some(s.name.clone()),
                        None => return Err("put the cursor on a declaration to move it"),
                    }
                } else {
                    None
                };
                (m.path.clone(), name)
            }
        };
        Ok(Self::new(from, name))
    }
    pub fn preview_effect(&self) -> Vec<Effect> {
        match self.current().map(|row| RelPath::from(row.path())) {
            Some(path) if self.preview.as_ref().map(|p| &p.path) != Some(&path) => {
                vec![Effect::Preview { path }]
            }
            _ => Vec::new(),
        }
    }
    pub fn site(&self) -> Option<(RelPath, u32)> {
        self.current().map(|row| (row.path().into(), row.line()))
    }
    pub fn scroll_focused(&self) -> bool {
        self.focus == MovePanel::Detail
    }
    pub fn previewed(&mut self, preview: FilePreview) {
        if self.current().is_none_or(|row| *row.path() != preview.path) {
            return;
        }
        if self.preview.as_ref().is_none_or(|p| p.path != preview.path) {
            self.detail_scroll = 0;
        }
        self.preview = Some(preview);
    }
    pub fn plan_failed(&mut self, message: String) {
        self.busy = false;
        self.applying = false;
        self.plan = None;
        self.error = Some(message);
    }
    pub fn failed(&mut self) {
        self.busy = false;
        self.applying = false;
    }
    pub fn toggle_diff(&mut self) {
        self.diff = !self.diff;
        self.detail_scroll = 0;
    }
    pub fn input(&mut self, c: Option<char>) {
        crate::input::TextInput::new(&mut self.to).edit(c);
    }
}
