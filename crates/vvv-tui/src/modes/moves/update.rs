//! Pure transitions of a file or symbol move.
use super::{MoveMode, MovePanel, MovePlan};
use crate::action::Effect;
use crate::model::{FilePreview, Panels};
use crate::modes::context::ModeContext;
use vvv_engine::protocol::FileChange;
use vvv_engine::{Intent, Notice, RelPath, Respelling, SymbolKind};
impl MoveMode {
    pub fn focus_by(&mut self, by: i32) {
        self.focus = self.focus.step(by);
    }
    pub fn focus_nth(&mut self, n: u8) {
        if let Some(p) = MovePanel::nth(n) {
            self.focus = p;
        };
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
        self.detail_scroll = (self.detail_scroll as i32 + by).max(0) as usize;
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
                Vec::new()
            }
        }
    }
    pub fn commit(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        match &self.plan {
            Some(plan) if !self.busy => {
                self.busy = true;
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
        self.last_list = if self.len(MovePanel::Respellings) > 0 {
            MovePanel::Respellings
        } else {
            MovePanel::Structural
        };
        self.preview_effect()
    }
    pub fn from_results(
        results: &crate::model::Results,
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
        self.preview = Some(preview);
        self.detail_scroll = 0;
    }
    pub fn plan_failed(&mut self, message: String) {
        self.busy = false;
        self.plan = None;
        self.error = Some(message);
    }
    pub fn failed(&mut self) {
        self.busy = false;
    }
    pub fn toggle_diff(&mut self) {
        self.diff = !self.diff;
        self.detail_scroll = 0;
    }
    pub fn input(&mut self, c: Option<char>) {
        crate::input::TextInput::new(&mut self.to).edit(c);
    }
}
