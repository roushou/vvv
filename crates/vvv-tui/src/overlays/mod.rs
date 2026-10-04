//! Overlay questions, selection, and typed rendering.
pub(crate) mod navigation;
pub(crate) mod places;
pub(crate) mod screen;
use crate::modes::search::{Category, Relation, filters::Restriction};
use crate::modes::search::{Search, query::Filter};
use crate::render::Painter;
use crate::screen::Screen;
use navigation::NavigationPicker;
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
    Places(places::Places),
    Menu(Menu),
    Navigation(NavigationPicker),
    Confirm(Confirm),
    /// Effective keys captured at the originating focus. Nested help retains
    /// the overlay it returns to; scrolling never changes the underlying mode.
    Help {
        title: String,
        previous: Option<Box<Overlay>>,
        sections: Vec<(
            &'static str,
            Vec<crate::keymap::Row<'static, crate::action::Action>>,
        )>,
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
    pub fn edit_input(&mut self, edit: crate::input::Edit<'_>) {
        match self {
            Self::Menu(m) => {
                if crate::input::TextInput::new(&mut m.filter, &mut m.caret).apply(edit) {
                    m.cursor = 0;
                }
            }
            Self::Places(p) => {
                if crate::input::TextInput::new(&mut p.filter, &mut p.caret).apply(edit) {
                    p.cursor.index = 0;
                }
            }
            Self::Navigation(p) => {
                if crate::input::TextInput::new(&mut p.filter, &mut p.caret).apply(edit) {
                    p.cursor.index = 0;
                }
            }
            _ => {}
        }
    }
    pub fn help_scrolled(&mut self, by: i32, viewport: (u16, u16)) -> bool {
        if let Self::Help {
            title,
            sections,
            scroll,
            ..
        } = self
        {
            let limit = HelpBox::new(title, sections, *scroll, Painter::plain())
                .scroll_limit(viewport.0, viewport.1.saturating_sub(1));
            *scroll = (*scroll)
                .min(limit)
                .saturating_add_signed(by as isize)
                .min(limit);
            true
        } else {
            false
        }
    }

    pub fn screen(&self) -> &'static Screen {
        match self {
            Self::Places(_) => &screen::PLACES_SCREEN,
            Self::Navigation(_) => &screen::NAVIGATION_SCREEN,
            Self::Menu(_) => &MENU_SCREEN,
            Self::Confirm(_) => &CONFIRM_SCREEN,
            Self::Help { .. } => &HELP_SCREEN,
            Self::Report { .. } => &REPORT_SCREEN,
        }
    }
    pub fn render(&self, painter: Painter, area: Rect, buf: &mut Buffer) {
        match self {
            Self::Places(places) => screen::PlacesBox::new(places, painter)
                .screen()
                .render(area, buf),
            Self::Navigation(picker) => screen::NavigationBox::new(picker, painter)
                .screen()
                .render(area, buf),
            Self::Menu(menu) => MenuBox::new(menu, painter).screen().render(area, buf),
            Self::Confirm(confirm) => ConfirmBox::new(confirm, painter).screen().render(area, buf),
            Self::Help {
                title,
                sections,
                scroll,
                ..
            } => HelpBox::new(title, sections, *scroll, painter)
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
    pub filter: String,
    pub caret: crate::input::Caret,
    pub selected: Option<String>,
    pub counted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTarget {
    Filters,
    Symbol,
    Language,
    Relation,
    Category,
    Location,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    /// The filter value, or `None` for "any".
    pub value: Option<String>,
    pub count: Option<usize>,
}

impl Menu {
    /// Symbol kinds, or the registered languages; `current` preselects.
    pub fn new(target: MenuTarget, values: Vec<String>, current: Option<&str>) -> Self {
        let mut items = vec![MenuItem {
            label: "any".to_owned(),
            value: None,
            count: None,
        }];
        items.extend(values.into_iter().map(|v| MenuItem {
            label: v.clone(),
            value: Some(v),
            count: None,
        }));
        let cursor = items
            .iter()
            .position(|i| i.value.as_deref() == current)
            .unwrap_or(0);
        Self {
            target,
            items,
            cursor,
            filter: String::new(),
            caret: Default::default(),
            selected: current.map(str::to_owned),
            counted: false,
        }
    }

    /// What the hub shows about the entered declaration.
    pub fn relations(current: Relation) -> Self {
        let items = Relation::ALL
            .iter()
            .map(|r| MenuItem {
                label: r.label().to_owned(),
                value: Some(r.key().to_owned()),
                count: None,
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
            filter: String::new(),
            caret: Default::default(),
            selected: Some(current.key().to_owned()),
            counted: false,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.target {
            MenuTarget::Filters => "search filters",
            MenuTarget::Symbol => "symbol kind",
            MenuTarget::Language => "language",
            MenuTarget::Relation => "relation",
            MenuTarget::Category => "result category",
            MenuTarget::Location => "look in",
        }
    }

    pub fn shown(&self) -> Vec<MenuItem> {
        let filter = self.filter.to_lowercase();
        let mut items: Vec<_> = self
            .items
            .iter()
            .filter(|i| i.label.to_lowercase().contains(&filter))
            .cloned()
            .collect();
        if self.target == MenuTarget::Location
            && !self.filter.is_empty()
            && !items
                .iter()
                .any(|i| i.value.as_deref() == Some(self.filter.as_str()))
        {
            items.push(MenuItem {
                label: format!("Use path: {}", self.filter),
                value: Some(self.filter.clone()),
                count: None,
            });
        }
        items
    }

    pub fn input(&mut self, c: Option<char>) {
        crate::input::TextInput::new(&mut self.filter, &mut self.caret).edit(c);
        self.cursor = 0;
    }

    pub fn move_cursor(&mut self, by: i32) {
        let last = self.shown().len().saturating_sub(1) as i32;
        self.cursor = (self.cursor as i32 + by).clamp(0, last) as usize;
    }

    pub fn for_target(
        target: MenuTarget,
        languages: &[String],
        search: &Search,
        counts_current: bool,
    ) -> Self {
        let mut menu = match target {
            MenuTarget::Filters => {
                let mut items: Vec<_> = Restriction::ALL
                    .iter()
                    .filter(|r| **r != Restriction::Files || search.results.has_file_list())
                    .map(|r| MenuItem {
                        label: format!(
                            "{}: {}",
                            r.label(),
                            r.value(search).unwrap_or_else(|| {
                                if *r == Restriction::Location {
                                    "workspace".into()
                                } else {
                                    "any".into()
                                }
                            })
                        ),
                        value: Some(r.key().to_owned()),
                        count: None,
                    })
                    .collect();
                items.push(MenuItem {
                    label: "Reset all filters · keep query".into(),
                    value: Some("all".into()),
                    count: None,
                });
                Self {
                    target,
                    items,
                    cursor: 0,
                    filter: String::new(),
                    caret: Default::default(),
                    selected: None,
                    counted: false,
                }
            }
            MenuTarget::Symbol => {
                let values = SymbolKind::ALL
                    .iter()
                    .map(|k| k.as_str().to_owned())
                    .collect();
                Self::new(target, values, search.query.filter(Filter::Symbol))
            }
            MenuTarget::Language => Self::new(
                target,
                languages.to_vec(),
                search.query.filter(Filter::Lang),
            ),
            MenuTarget::Relation => Self::relations(search.results.relation),
            MenuTarget::Category => {
                let items = Category::ALL
                    .iter()
                    .map(|c| MenuItem {
                        label: c.label().to_owned(),
                        value: Some(c.key().to_owned()),
                        count: None,
                    })
                    .collect();
                Self {
                    target,
                    items,
                    cursor: Category::ALL
                        .iter()
                        .position(|c| *c == search.results.category)
                        .unwrap_or(0),
                    filter: String::new(),
                    caret: Default::default(),
                    selected: Some(search.results.category.key().to_owned()),
                    counted: false,
                }
            }
            MenuTarget::Location => Self::new(
                target,
                search.locations.choices(),
                search.locations.selected.as_ref().map(|p| p.as_str()),
            ),
        };
        match target {
            MenuTarget::Symbol => menu.items[0].label = "Any symbol kind".into(),
            MenuTarget::Language => menu.items[0].label = "Any language".into(),
            MenuTarget::Location => menu.items[0].label = "Entire workspace".into(),
            _ => {}
        }
        menu.counted = counts_current
            && matches!(
                target,
                MenuTarget::Symbol
                    | MenuTarget::Language
                    | MenuTarget::Location
                    | MenuTarget::Category
            );
        if menu.counted {
            menu.count(search);
        }
        menu
    }
    /// Tally the loaded answer once; opening a location picker must not scan
    /// every hit again for every suggested directory.
    fn count(&mut self, search: &Search) {
        let mut counts: std::collections::BTreeMap<Option<String>, usize> = Default::default();
        for m in search.results.matches.iter() {
            if self.target != MenuTarget::Location && !search.locations.includes(&m.path) {
                continue;
            }
            *counts.entry(None).or_default() += 1;
            match self.target {
                MenuTarget::Location => {
                    let mut prefix = String::new();
                    for component in m.path.as_str().split('/') {
                        if !prefix.is_empty() {
                            prefix.push('/');
                        }
                        prefix.push_str(component);
                        *counts.entry(Some(prefix.clone())).or_default() += 1;
                    }
                }
                MenuTarget::Symbol => {
                    if let Some(symbol) = &m.symbol {
                        *counts
                            .entry(Some(symbol.kind.as_str().to_owned()))
                            .or_default() += 1;
                    }
                }
                MenuTarget::Language => {
                    *counts
                        .entry(Some(m.language.as_str().to_owned()))
                        .or_default() += 1;
                }
                MenuTarget::Category => {
                    for c in Category::ALL.iter().filter(|c| c.includes(m.role)) {
                        *counts.entry(Some(c.key().to_owned())).or_default() += 1;
                    }
                }
                _ => {}
            }
        }
        for item in &mut self.items {
            item.count = Some(counts.get(&item.value).copied().unwrap_or(0));
        }
    }

    pub fn chosen(&self) -> Option<(MenuTarget, Option<String>)> {
        self.current().map(|i| (self.target, i.value))
    }

    pub fn current(&self) -> Option<MenuItem> {
        self.shown().get(self.cursor).cloned()
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
