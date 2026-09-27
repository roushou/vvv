//! Overlay questions, selection, and typed rendering.
pub(crate) mod screen;
mod update;
use crate::modes::search::Relation;
use crate::screen::Screen;
use vvv_engine::report::Document;

/// A small question in front of the mode.
#[derive(Debug, Clone)]
pub enum Overlay {
    Menu(Menu),
    Confirm(Confirm),
    /// The key list: the screen the user was in and the panel that had the
    /// focus, kept so the list stays about them; `scroll` is how many rows
    /// are above the box.
    Help {
        screen: &'static Screen,
        focus: usize,
        scroll: usize,
    },
    /// What an apply produced, as the report the picker shows.
    Report {
        report: Box<Document>,
        /// Where the cursor stands among the report's source rows.
        cursor: usize,
    },
}

// ---------------------------------------------------------------- overlays

/// A list to pick one value from; the choice edits the query bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub target: MenuTarget,
    pub items: Vec<MenuItem>,
    pub cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTarget {
    Symbol,
    Language,
    Relation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    /// The filter value, or `None` for "any".
    pub value: Option<String>,
}

impl Menu {
    /// Symbol kinds, or the registered languages; `current` preselects.
    pub fn new(target: MenuTarget, values: Vec<String>, current: Option<&str>) -> Self {
        let mut items = vec![MenuItem {
            label: "any".to_owned(),
            value: None,
        }];
        items.extend(values.into_iter().map(|v| MenuItem {
            label: v.clone(),
            value: Some(v),
        }));
        let cursor = items
            .iter()
            .position(|i| i.value.as_deref() == current)
            .unwrap_or(0);
        Self {
            target,
            items,
            cursor,
        }
    }

    /// What the hub shows about the entered declaration.
    pub fn relations(current: Relation) -> Self {
        let items = Relation::ALL
            .iter()
            .map(|r| MenuItem {
                label: r.label().to_owned(),
                value: Some(r.key().to_owned()),
            })
            .collect();
        let cursor = Relation::ALL
            .iter()
            .position(|r| *r == current)
            .unwrap_or(0);
        Self {
            target: MenuTarget::Relation,
            items,
            cursor,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.target {
            MenuTarget::Symbol => "symbol kind",
            MenuTarget::Language => "language",
            MenuTarget::Relation => "relation",
        }
    }

    pub fn current(&self) -> &MenuItem {
        &self.items[self.cursor]
    }

    pub fn move_cursor(&mut self, by: i32) {
        let last = self.items.len() as i32 - 1;
        self.cursor = (self.cursor as i32 + by).clamp(0, last) as usize;
    }
}

/// A yes/no question before something irreversible-ish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub question: String,
    pub then: Confirmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmed {
    Undo,
}
