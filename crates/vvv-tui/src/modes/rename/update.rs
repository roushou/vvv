//! Pure transitions of a rename.
use super::{RenameMode, RenamePanel, RenameTarget};
use crate::action::Effect;
use crate::model::{FilePreview, Panels};
use crate::modes::context::ModeContext;
use vvv_engine::protocol::FileChange;
use vvv_engine::{Confidence, Intent, Match, Occurrence, RelPath, RenameIntent, Selection};
impl RenameMode {
    pub fn input(&mut self, c: Option<char>) {
        crate::input::TextInput::new(&mut self.name).edit(c);
    }
    pub fn focus_by(&mut self, by: i32) {
        self.focus = self.focus.step(by);
    }
    pub fn focus_nth(&mut self, n: u8) {
        if let Some(p) = RenamePanel::nth(n) {
            self.focus = p;
        };
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
        self.detail_scroll = (self.detail_scroll as i32 + by).max(0) as usize;
    }
    pub fn plan(&mut self, generation: u64, debounce: bool) -> Vec<Effect> {
        if self.name.trim().is_empty() {
            self.changes.clear();
            self.error = None;
            self.busy = false;
            return Vec::new();
        }
        let mut intent = RenameIntent::new(&self.target.name, self.name.trim());
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
        let to = self.name.trim().to_owned();
        if to.is_empty() || to == self.target.name {
            return context.fail("type the new name first");
        }
        if self.ticks.is_empty() {
            return context.fail("nothing ticked");
        }
        let mut intent =
            RenameIntent::new(&self.target.name, to).selecting(Selection::Ids(self.ticks.clone()));
        intent.references.symbol = self.target.symbol;
        intent.references.language = self.language.clone();
        intent.references.declared_in = self.target.declared_in.clone();
        self.busy = true;
        context.status.busy = true;
        vec![Effect::Commit {
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
    pub fn toggled(&mut self, all: bool) -> Vec<Effect> {
        if all {
            self.toggle_panel();
        } else {
            self.toggle();
            self.moved(1);
            return self.preview_effect();
        }

        Vec::new()
    }
    pub fn from_results(results: &crate::model::Results) -> Result<Self, &'static str> {
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
        self.preview = Some(preview);
        self.detail_scroll = 0;
    }
    pub fn plan_failed(&mut self, message: String) {
        self.busy = false;
        self.error = Some(message);
    }
    pub fn failed(&mut self) {
        self.busy = false;
    }
    pub fn judgment(&self) -> RenameIntent {
        let mut intent = RenameIntent::new(&self.target.name, &self.target.name);
        intent.references.symbol = self.target.symbol;
        intent.references.language = self.language.clone();
        intent.references.declared_in = self.target.declared_in.clone();
        intent
    }
}
