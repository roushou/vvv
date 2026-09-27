//! History selection and undo eligibility.
pub(crate) mod screen;
mod update;
use crate::model::{Cursor, Panels};
use vvv_engine::HistoryEntry;
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
}
