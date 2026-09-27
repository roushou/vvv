//! History selection and undo eligibility.
pub(crate) mod screen;
use crate::action::{Action, Effect};
use crate::model::{Cursor, Panels};
use crate::modes::context::ModeContext;
use crate::overlays::{Confirm, Confirmed};
use vvv_engine::HistoryEntry;
use vvv_engine::protocol::vocabulary::IntentLine;
// ---------------------------------------------------------------- history

#[derive(Debug)]
pub struct HistoryMode {
    pub entries: Vec<HistoryEntry>,
    pub cursor: Cursor,
    pub focus: HistoryPanel,
    pub files_scroll: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryPanel {
    Entries,
    Files,
}

impl Panels for HistoryPanel {
    const ALL: &'static [Self] = &[Self::Entries, Self::Files];
}

impl HistoryMode {
    pub fn new(entries: Vec<HistoryEntry>) -> Self {
        let cursor = Cursor {
            index: entries.len().saturating_sub(1),
        };
        Self {
            entries,
            cursor,
            focus: HistoryPanel::Entries,
            files_scroll: 0,
        }
    }

    pub fn current(&self) -> Option<&HistoryEntry> {
        self.entries.get(self.cursor.index)
    }

    pub fn is_newest(&self) -> bool {
        self.cursor.index + 1 == self.entries.len()
    }

    pub fn update(&mut self, action: Action, _context: &mut ModeContext<'_>) -> Vec<Effect> {
        match action {
            Action::FocusNext => self.focus_by(1),
            Action::FocusPrev => self.focus_by(-1),
            Action::FocusNth(n) => self.focus_nth(n),
            Action::Move(n) => self.moved(n),
            Action::Top => self.moved(i32::MIN / 2),
            Action::Bottom => self.moved(i32::MAX / 2),
            Action::Scroll(n) if self.scroll_focused() => {
                self.scrolled(n);
                return Vec::new();
            }
            Action::Scroll(n) => self.moved(n),
            _ => return Vec::new(),
        }
        Vec::new()
    }
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
