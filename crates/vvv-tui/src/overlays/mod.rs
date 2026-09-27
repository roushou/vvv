//! Overlay questions, selection, and typed rendering.
pub(crate) mod screen;
use crate::modes::search::Relation;
use crate::modes::search::{Search, query::Filter};
use crate::render::Painter;
use crate::screen::Screen;
use ratatui::{buffer::Buffer, layout::Rect};
use screen::{
    CONFIRM_SCREEN, ConfirmBox, HELP_SCREEN, HelpBox, MENU_SCREEN, MenuBox, REPORT_SCREEN,
    ReportBox,
};
use vvv_engine::report::Document;
use vvv_engine::report::{Detailed, Options, Source, View};
use vvv_engine::{RelPath, SymbolKind};

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

impl Overlay {
    fn report_sites(&self) -> Vec<Source> {
        let Self::Report { report, .. } = self else {
            return Vec::new();
        };
        Detailed
            .present(report, Options::default(), usize::MAX)
            .body
            .into_iter()
            .filter_map(|row| row.source)
            .collect()
    }
    pub fn report_moved(&mut self, by: i32) {
        let last = self.report_sites().len().saturating_sub(1) as i32;
        if let Self::Report { cursor, .. } = self {
            *cursor = (*cursor as i32 + by).clamp(0, last) as usize;
        }
    }
    pub fn report_site(&self) -> Option<(RelPath, u32)> {
        let Self::Report { cursor, .. } = self else {
            return None;
        };
        self.report_sites()
            .get(*cursor)
            .map(|site| (site.path.clone(), site.line))
    }
    pub fn help_scrolled(&mut self, by: i32) -> bool {
        if let Self::Help { scroll, .. } = self {
            *scroll = (*scroll as i32 + by).max(0) as usize;
            true
        } else {
            false
        }
    }

    pub fn screen(&self) -> &'static Screen {
        match self {
            Self::Menu(_) => &MENU_SCREEN,
            Self::Confirm(_) => &CONFIRM_SCREEN,
            Self::Help { .. } => &HELP_SCREEN,
            Self::Report { .. } => &REPORT_SCREEN,
        }
    }
    pub fn render(&self, painter: Painter, area: Rect, buf: &mut Buffer) {
        match self {
            Self::Menu(menu) => MenuBox::new(menu, painter).screen().render(area, buf),
            Self::Confirm(confirm) => ConfirmBox::new(confirm, painter).screen().render(area, buf),
            Self::Help {
                screen,
                focus,
                scroll,
            } => HelpBox::new(screen, *focus, *scroll, painter)
                .screen()
                .render(area, buf),
            Self::Report { report, cursor } => ReportBox::new(report, *cursor, painter)
                .screen()
                .render(area, buf),
        }
    }
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

    pub fn for_target(target: MenuTarget, languages: &[String], search: &Search) -> Self {
        match target {
            MenuTarget::Symbol => {
                let values = SymbolKind::ALL
                    .iter()
                    .map(|k| k.as_str().to_owned())
                    .collect();
                let current = search.query.filter(Filter::Symbol).map(str::to_owned);
                Self::new(target, values, current.as_deref())
            }
            MenuTarget::Language => {
                let current = search.query.filter(Filter::Lang).map(str::to_owned);
                Self::new(target, languages.to_vec(), current.as_deref())
            }
            MenuTarget::Relation => Self::relations(search.results.relation),
        }
    }
    pub fn chosen(&self) -> (MenuTarget, Option<String>) {
        (self.target, self.current().value.clone())
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
