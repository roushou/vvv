//! Pure history transitions.
use super::{HistoryMode, HistoryPanel};
use crate::model::{Confirm, Confirmed, Panels};
use vvv_engine::protocol::vocabulary::IntentLine;
impl HistoryMode {
    pub fn focus_by(&mut self, by: i32) {
        self.focus = self.focus.step(by);
    }
    pub fn focus_nth(&mut self, n: u8) {
        if let Some(p) = HistoryPanel::nth(n) {
            self.focus = p;
        };
    }
    pub fn moved(&mut self, by: i32) {
        let len = self.entries.len();
        self.cursor.move_by(by, len);
        self.files_scroll = 0;
    }
    pub fn scrolled(&mut self, by: i32) {
        self.files_scroll = (self.files_scroll as i32 + by).max(0) as usize;
    }
    pub fn scroll_focused(&self) -> bool {
        self.focus == HistoryPanel::Files
    }
    pub fn confirmation(&self) -> Result<Confirm, &'static str> {
        match self.current() {
            Some(entry) if self.is_newest() => Ok(Confirm {
                question: format!("↩ #{}  {}?", entry.id, IntentLine(&entry.intent)),
                then: Confirmed::Undo,
            }),
            Some(_) => Err("only the newest entry can be undone"),
            None => Err("nothing to undo"),
        }
    }
}
