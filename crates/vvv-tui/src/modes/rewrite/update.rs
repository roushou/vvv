//! Pure rewrite transitions.
use super::{RewriteMode, RewritePanel};
use crate::action::Effect;
use crate::model::Panels;
use crate::modes::context::ModeContext;
use vvv_engine::protocol::FileChange;
use vvv_engine::{Intent, RelPath, Selection};
impl RewriteMode {
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
        self.detail_scroll = (self.detail_scroll as i32 + by).max(0) as usize;
    }
    pub fn plan(&mut self, generation: u64) -> Vec<Effect> {
        match self.intent() {
            Some(intent) => {
                self.busy = true;
                vec![Effect::Plan {
                    generation,
                    intent: Intent::Rewrite(intent),
                    debounce: true,
                }]
            }
            None => {
                self.changes.clear();
                self.error = None;
                self.busy = false;
                Vec::new()
            }
        }
    }
    pub fn commit(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        if self.busy || self.changes.is_empty() {
            return context.fail("type a template first");
        }
        if self.ticks.is_empty() {
            return context.fail("nothing ticked");
        }
        let Some(intent) = self.intent() else {
            return Vec::new();
        };
        self.busy = true;
        context.status.busy = true;
        vec![Effect::Commit {
            intent: Intent::Rewrite(intent.selecting(Selection::Ids(self.ticks.clone()))),
        }]
    }
    pub fn planned(&mut self, files: Vec<FileChange>) -> Vec<Effect> {
        self.changes = files;
        self.error = None;
        self.busy = false;
        Vec::new()
    }
    pub fn toggled(&mut self, all: bool) -> Vec<Effect> {
        if all {
            self.toggle_all();
        } else {
            self.toggle();
            self.moved(1);
            return Vec::new();
        }
        Vec::new()
    }
    pub fn from_results(results: &crate::model::Results) -> Result<Self, &'static str> {
        let Some(query) = results.query.clone() else {
            return Err("search for the pattern to rewrite first");
        };
        // Rewrite is query-scoped: its ticks must be a subset of what the
        // query searches for, not the (possibly wider) anchored occurrences.
        let matches = results.matches.clone();
        if matches.is_empty() {
            return Err("nothing matched; a rewrite acts on the matches");
        }
        Ok(Self::new(query, matches))
    }
    pub fn scroll_focused(&self) -> bool {
        self.focus == RewritePanel::Detail
    }
    pub fn site(&self) -> Option<(RelPath, u32)> {
        self.current().map(|m| (m.path.clone(), m.start.line))
    }
    pub fn plan_failed(&mut self, message: String) {
        self.busy = false;
        self.changes.clear();
        self.error = Some(message);
    }
    pub fn failed(&mut self) {
        self.busy = false;
    }
    pub fn input(&mut self, c: Option<char>) {
        crate::input::TextInput::new(&mut self.template).edit(c);
    }
}
