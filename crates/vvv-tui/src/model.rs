//! All TUI state as plain data: the search hub, the mode in front of it,
//! and an overlay when a small question is open. Nothing here does I/O or
//! knows about terminals; see `update.rs` for how it changes and `render/`
//! for how it looks.

use vvv_engine::RelPath;

use vvv_engine::report::{Detailed, Document, Options, View};
use vvv_engine::{
    Confidence, Consumer, Deps, Explanation, Highlight, Impact, Match, Occurrence, Query,
    References, ReferencesQuery, Role,
};

use super::action::Action;
use super::keymap::{Dispatch, Layer, When};
use super::query::QueryBar;
use super::screen::{Screen, overlay, search};
pub(crate) use crate::modes::history::HistoryMode;
use crate::modes::history::screen as history;
pub(crate) use crate::modes::moves::MoveMode;
use crate::modes::moves::screen as moving;
use crate::modes::rename::screen as rename;
pub(crate) use crate::modes::rename::{RenameMode, RenameTarget};
pub(crate) use crate::modes::rewrite::RewriteMode;
use crate::modes::rewrite::screen as rewrite;

#[derive(Debug)]
pub struct Model {
    pub root: String,
    /// Ids of the registered languages, for the language menu.
    pub languages: Vec<String>,
    /// The hub; kept while a mode is open so `esc` comes back to it intact.
    pub search: Search,
    pub mode: Mode,
    /// A mode was entered and its first plan is still out: keys already go
    /// to it, but the screen stays on search until there is something to
    /// show, so the layout appears filled instead of empty then filled.
    pub arriving: bool,
    pub overlay: Option<Overlay>,
    /// How a report's rows are laid out; the picker's `v` toggles it.
    pub view: ReportView,
    /// Width of the left column as a percentage of the body.
    pub split: u16,
    pub status: Status,
    /// Bumps on every request whose answer may be superseded (search, plan).
    pub generation: u64,
    pub quit: bool,
}

/// How the picker lays a report's rows out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReportView {
    /// One tight row per hit, as the modes have always shown them.
    #[default]
    Compact,
    /// The terminal's layout: file headers, ordinals, source, addresses.
    Detailed,
}

impl ReportView {
    /// The engine view this choice draws with.
    pub fn view(self) -> Box<dyn vvv_engine::report::View> {
        match self {
            Self::Compact => Box::new(crate::view::Compact),
            Self::Detailed => Box::new(vvv_engine::report::Detailed),
        }
    }

    /// The other view.
    pub fn toggled(self) -> Self {
        match self {
            Self::Compact => Self::Detailed,
            Self::Detailed => Self::Compact,
        }
    }
}

impl Model {
    /// The mode the screen draws: the one entered, once it has answered.
    pub fn shown(&self) -> &Mode {
        if self.arriving {
            &Mode::Search
        } else {
            &self.mode
        }
    }

    /// Whether a binding's condition holds now.
    pub fn holds(&self, when: When) -> bool {
        match when {
            When::Always => true,
            When::QueryEmpty => self.search.query.is_empty(),
            When::QueryNotEmpty => !self.search.query.is_empty(),
            When::Anchored => self.search.results.is_anchored(),
        }
    }

    /// The sources a report's rows stand for, in the order it draws them.
    fn report_sites(report: &Document) -> Vec<vvv_engine::report::Source> {
        Detailed
            .present(report, Options::default(), usize::MAX)
            .body
            .into_iter()
            .filter_map(|row| row.source)
            .collect()
    }

    /// Move the report overlay's cursor by `by` source rows.
    pub fn report_moved(&mut self, by: i32) {
        if let Some(Overlay::Report { report, cursor }) = &mut self.overlay {
            let last = Self::report_sites(report).len().saturating_sub(1) as i32;
            *cursor = (*cursor as i32 + by).clamp(0, last) as usize;
        }
    }

    /// The source the report overlay's cursor stands on, when it is a report.
    pub fn report_site(&self) -> Option<(RelPath, u32)> {
        let Some(Overlay::Report { report, cursor }) = &self.overlay else {
            return None;
        };
        Self::report_sites(report)
            .get(*cursor)
            .map(|site| (site.path.clone(), site.line))
    }

    /// The view of the mode on screen.
    pub fn mode_screen(&self) -> &'static Screen {
        match self.shown() {
            Mode::Search => &search::SEARCH,
            Mode::Rename(_) => &rename::RENAME,
            Mode::Move(_) => &moving::MOVE,
            Mode::Rewrite(_) => &rewrite::REWRITE,
            Mode::History(_) => &history::HISTORY,
        }
    }

    /// The overlay's view, when one is open.
    pub fn overlay_screen(&self) -> Option<&'static Screen> {
        match &self.overlay {
            Some(Overlay::Menu(_)) => Some(&overlay::MENU_SCREEN),
            Some(Overlay::Confirm(_)) => Some(&overlay::CONFIRM_SCREEN),
            Some(Overlay::Help { .. }) => Some(&overlay::HELP_SCREEN),
            Some(Overlay::Report { .. }) => Some(&overlay::REPORT_SCREEN),
            None => None,
        }
    }

    /// The view the keys go to: the overlay when one is open, the mode otherwise.
    pub fn screen(&self) -> &'static Screen {
        self.overlay_screen().unwrap_or_else(|| self.mode_screen())
    }

    /// The focused panel's index within the active view.
    pub fn focus(&self) -> usize {
        match &self.overlay {
            Some(_) => 0,
            None => self.shown().focus_index(self.search.focus),
        }
    }

    /// The word the status bar shows for a row whose meaning depends on
    /// the state: what `⏎` applies, which entry `u` undoes, what `d` shows.
    pub fn describe(&self, dispatch: Dispatch<Action>) -> String {
        let files = |n: usize| format!("{n} file{}", if n == 1 { "" } else { "s" });
        let Dispatch::Run(action) = dispatch else {
            return String::new();
        };
        match (action, self.shown()) {
            (Action::Enter, Mode::Rename(r)) => {
                format!("apply {} in {}", r.ticks.len(), files(r.files()))
            }
            (Action::Enter, Mode::Rewrite(rw)) => {
                format!("apply {} in {}", rw.ticks.len(), files(rw.files()))
            }
            (Action::Enter, Mode::Move(mv)) => match &mv.plan {
                Some(plan) => format!("apply {}", files(plan.files.len())),
                None => "apply".to_owned(),
            },
            (Action::Enter, _) => "apply".to_owned(),
            (Action::Undo, Mode::History(h)) => h
                .entries
                .last()
                .map_or("undo".to_owned(), |e| format!("undo #{}", e.id)),
            (Action::Undo, _) => "undo".to_owned(),
            (Action::Diff, Mode::Move(mv)) if mv.diff => "source".to_owned(),
            (Action::Diff, _) => "diff".to_owned(),
            _ => String::new(),
        }
    }

    pub fn new(root: String, languages: Vec<String>) -> Self {
        Self {
            root,
            languages,
            search: Search::default(),
            mode: Mode::Search,
            arriving: false,
            overlay: None,
            view: ReportView::default(),
            split: 50,
            status: Status::default(),
            generation: 0,
            quit: false,
        }
    }

    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }
}

/// One job, with its own layout and keys.
#[derive(Debug)]
pub enum Mode {
    Search,
    Rename(Box<RenameMode>),
    Move(Box<MoveMode>),
    Rewrite(Box<RewriteMode>),
    History(HistoryMode),
}

impl Mode {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Rename(_) => "rename",
            Self::Move(_) => "move",
            Self::Rewrite(_) => "rewrite",
            Self::History(_) => "history",
        }
    }

    /// The focused panel's index in the view that draws this mode.
    pub fn focus_index(&self, search: SearchPanel) -> usize {
        match self {
            Self::Search => search.index(),
            Self::Rename(r) => r.focus.index(),
            Self::Move(mv) => mv.focus.index(),
            Self::Rewrite(rw) => rw.focus.index(),
            Self::History(h) => h.focus.index(),
        }
    }
}

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

/// A list panel's cursor. Rows live in the mode; the cursor only knows how
/// to stay inside them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    pub index: usize,
}

impl Cursor {
    pub fn move_by(&mut self, by: i32, len: usize) {
        if len == 0 {
            self.index = 0;
            return;
        }
        let last = len as i32 - 1;
        self.index = (self.index as i32).saturating_add(by).clamp(0, last) as usize;
    }

    pub fn clamp(&mut self, len: usize) {
        self.index = self.index.min(len.saturating_sub(1));
    }
}

/// What kind of panel is focused, which selects the keys it shares with
/// every other panel of that kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    /// Typing goes here: the query, a new name, a destination, a template.
    Input,
    /// Rows with a cursor.
    List,
    /// Text that scrolls: a file, a diff.
    Text,
}

impl PanelKind {
    /// The keys a panel of this kind shares; `None` for an input.
    pub fn layer(self) -> Option<&'static Layer<Action>> {
        match self {
            Self::Input => None,
            Self::List => Some(&super::screen::defaults::LIST),
            Self::Text => Some(&super::screen::defaults::TEXT),
        }
    }
}

/// Focus order within a mode: `tab` walks it, `1`–`5` jump into it.
pub trait Panels: Copy + PartialEq + Sized + 'static {
    const ALL: &'static [Self];

    /// The position of this panel in the focus order.
    fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// The panel `by` steps away, wrapping: how the arrow keys move focus.
    fn step(self, by: i32) -> Self {
        if by > 0 { self.next() } else { self.prev() }
    }

    fn nth(n: u8) -> Option<Self> {
        Self::ALL.get(usize::from(n).checked_sub(1)?).copied()
    }
}

// ---------------------------------------------------------------- search

/// The hub: a query, its results, and context for the cursor row.
#[derive(Debug, Default)]
pub struct Search {
    pub query: QueryBar,
    pub results: Results,
    pub focus: SearchPanel,
    pub preview: Option<FilePreview>,
    /// A manual context scroll position; `None` follows the cursor.
    pub preview_scroll: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchPanel {
    #[default]
    Query,
    Results,
    Context,
}

impl Panels for SearchPanel {
    const ALL: &'static [Self] = &[Self::Query, Self::Results, Self::Context];
}

/// What the hub shows about the declaration it was narrowed to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Relation {
    /// Every judged occurrence, grouped by verdict.
    #[default]
    References,
    /// Only the occurrences that resolve to the declaration.
    Resolved,
    /// Only the ones syntax could not place.
    Unresolved,
    /// Only the ones belonging to another declaration.
    Other,
    /// The modules that import it, then those, outward.
    Impact,
    /// Where the declaration is: its address, reach and importers.
    Definition,
    /// The declaring file's imports, and who imports it.
    Deps,
}

impl Relation {
    pub const ALL: &'static [Self] = &[
        Self::References,
        Self::Resolved,
        Self::Unresolved,
        Self::Other,
        Self::Impact,
        Self::Definition,
        Self::Deps,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::References => "references",
            Self::Resolved => "✓ safe",
            Self::Unresolved => "? unverified",
            Self::Other => "✗ another declaration's",
            Self::Impact => "impact (importers)",
            Self::Definition => "definition",
            Self::Deps => "deps (imports)",
        }
    }

    /// The stable key a menu item carries, and reads back.
    pub fn key(self) -> &'static str {
        match self {
            Self::References => "references",
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
            Self::Other => "other",
            Self::Impact => "impact",
            Self::Definition => "definition",
            Self::Deps => "deps",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|r| r.key() == key)
    }

    /// The verdict a references view keeps, when it is narrowed to one.
    pub fn confidence(self) -> Option<Confidence> {
        match self {
            Self::Resolved => Some(Confidence::Resolved),
            Self::Unresolved => Some(Confidence::Unresolved),
            Self::Other => Some(Confidence::Other),
            Self::References | Self::Impact | Self::Definition | Self::Deps => None,
        }
    }

    pub fn is_impact(self) -> bool {
        matches!(self, Self::Impact)
    }

    /// Whether the relation lists the subject's occurrences.
    pub fn is_references(self) -> bool {
        matches!(
            self,
            Self::References | Self::Resolved | Self::Unresolved | Self::Other
        )
    }
}

/// Search results in the engine's order — declarations first — with the
/// cursor. A row can be *entered* as the search's subject: the rows then
/// become that declaration's judged occurrences instead of its spellings.
#[derive(Debug, Default)]
pub struct Results {
    pub query: Option<Query>,
    pub matches: Vec<Match>,
    /// The declaration the rows were narrowed to, when a row was entered:
    /// name, kind and file, exactly what `references` disambiguates by.
    pub subject: Option<ReferencesQuery>,
    /// What is shown about the subject.
    pub relation: Relation,
    /// The subject's occurrences once the engine answered; `None` while the
    /// request is out.
    pub references: Option<References>,
    /// The subject's consumer modules, once `impact` was asked for.
    pub impact: Option<Impact>,
    /// The declaration's definition, once `definition` was asked for.
    pub definition: Option<Explanation>,
    /// The declaring file's imports and importers, once `deps` was asked for.
    pub deps: Option<Deps>,
    pub cursor: Cursor,
}

impl Results {
    pub fn replace(&mut self, matches: Vec<Match>) {
        self.matches = matches;
        self.subject = None;
        self.references = None;
        self.clear_lenses();
        self.cursor.clamp(self.matches.len());
    }

    /// The engine's answer for a declaration entered: the subject is read
    /// off the answer and the rows become its references. Nothing changes
    /// on screen until this arrives, so the pane never blanks mid-request.
    pub fn entered(&mut self, references: References) {
        self.subject = Self::subject_of(&references);
        self.relation = Relation::References;
        self.references = Some(references);
        self.clear_lenses();
        self.cursor = Cursor::default();
    }

    /// The subject's consumer modules.
    pub fn show_impact(&mut self, impact: Impact) {
        self.relation = Relation::Impact;
        self.impact = Some(impact);
        self.cursor = Cursor::default();
    }

    /// Where the subject is declared: its address, reach and importers.
    pub fn show_definition(&mut self, definition: Explanation) {
        self.relation = Relation::Definition;
        self.definition = Some(definition);
        self.cursor = Cursor::default();
    }

    /// The declaring file's imports and importers.
    pub fn show_deps(&mut self, deps: Deps) {
        self.relation = Relation::Deps;
        self.deps = Some(deps);
        self.cursor = Cursor::default();
    }

    /// Forget every lens but the references, called when entering anew.
    fn clear_lenses(&mut self) {
        self.impact = None;
        self.definition = None;
        self.deps = None;
    }

    /// The subject a `references` answer is about: the declaration it
    /// opened with, named and placed.
    fn subject_of(references: &References) -> Option<ReferencesQuery> {
        let declaration = references.declarations.first()?;
        let symbol = declaration.symbol.as_ref()?;
        let mut query = ReferencesQuery::new(references.name.as_str())
            .of_symbol(symbol.kind)
            .declared_in(declaration.path.clone());
        query.language = Some(declaration.language.clone());
        Some(query)
    }

    /// Show another relation for the same subject; the cursor starts over.
    pub fn set_relation(&mut self, relation: Relation) {
        self.relation = relation;
        self.cursor = Cursor::default();
    }

    /// Leave the subject: the rows are the search's spellings again.
    pub fn leave(&mut self) {
        self.subject = None;
        self.references = None;
        self.clear_lenses();
        self.cursor.clamp(self.matches.len());
    }

    pub fn is_anchored(&self) -> bool {
        self.subject.is_some()
    }

    /// The subject's declaration, once references have answered: the row a
    /// `definition`, `deps` or jump is about.
    pub fn subject_declaration(&self) -> Option<&Match> {
        self.references.as_ref()?.declarations.first()
    }

    /// The occurrences the current relation keeps, in engine order.
    fn shown(&self) -> Vec<&Occurrence> {
        if !self.relation.is_references() {
            return Vec::new();
        }
        let Some(r) = &self.references else {
            return Vec::new();
        };
        match self.relation.confidence() {
            Some(confidence) => r
                .occurrences
                .iter()
                .filter(|o| o.confidence == confidence)
                .collect(),
            None => r.occurrences.iter().collect(),
        }
    }

    /// How many rows the cursor walks: the search's matches, or the
    /// subject's rows once anchored.
    pub fn len(&self) -> usize {
        if !self.is_anchored() {
            return self.matches.len();
        }
        if self.relation.is_impact() {
            self.impact.as_ref().map_or(0, |i| i.consumers.len())
        } else {
            self.shown().len()
        }
    }

    pub fn current(&self) -> Option<&Match> {
        let i = self.cursor.index;
        if !self.is_anchored() {
            return self.matches.get(i);
        }
        if self.relation.is_impact() {
            return None;
        }
        self.shown().get(i).map(|o| &o.m)
    }

    /// The module under the cursor, in the impact view.
    pub fn current_consumer(&self) -> Option<&Consumer> {
        if !self.relation.is_impact() {
            return None;
        }
        self.impact
            .as_ref()
            .and_then(|i| i.consumers.get(self.cursor.index))
    }

    /// Where the cursor stands: a match's line, a consumer's or the
    /// definition's file. What the context panel and `$EDITOR` open.
    pub fn current_site(&self) -> Option<(RelPath, u32)> {
        if let Some(consumer) = self.current_consumer() {
            return Some((consumer.path.clone(), 0));
        }
        match self.relation {
            Relation::Definition => self
                .definition
                .as_ref()
                .map(|e| (e.path.clone(), e.declared.map_or(0, |p| p.line))),
            Relation::Deps => self.deps.as_ref().map(|d| (d.path.clone(), 0)),
            _ => self.current().map(|m| (m.path.clone(), m.start.line)),
        }
    }

    /// The declaration a use row names, as an index into the current list:
    /// what the jump key moves the cursor to.
    pub fn declaration_row(&self) -> Option<usize> {
        if self.is_anchored() {
            return self
                .shown()
                .iter()
                .position(|o| o.m.role == Role::Declaration);
        }
        let current = self.current()?;
        let declaration = self.declaration_of(current)?;
        self.matches.iter().position(|m| m.id == declaration.id)
    }

    /// The subject's declarations, once anchored.
    pub fn anchored_declarations(&self) -> &[Match] {
        self.references
            .as_ref()
            .map_or(&[], |r| r.declarations.as_slice())
    }

    pub fn declarations(&self) -> impl Iterator<Item = &Match> {
        self.matches.iter().filter(|m| m.role == Role::Declaration)
    }

    /// The declaration a use row belongs to, when the results hold exactly
    /// one declaration of that name.
    pub fn declaration_of(&self, m: &Match) -> Option<&Match> {
        let mut same = self
            .declarations()
            .filter(|d| d.symbol.as_ref().is_some_and(|s| s.name == m.text));
        let first = same.next()?;
        same.next().is_none().then_some(first)
    }

    /// The declaration a row would enter: a declaration's own name, kind
    /// and file; a use row resolves through a unique same-named declaration.
    pub fn subject_at(&self, m: &Match) -> Option<ReferencesQuery> {
        let mut query = match &m.symbol {
            Some(s) => ReferencesQuery::new(s.name.as_str())
                .of_symbol(s.kind)
                .declared_in(m.path.clone()),
            None => {
                let d = self.declaration_of(m)?;
                let s = d.symbol.as_ref()?;
                ReferencesQuery::new(s.name.as_str())
                    .of_symbol(s.kind)
                    .declared_in(d.path.clone())
            }
        };
        query.language = self.query.as_ref().and_then(|q| q.language().cloned());
        Some(query)
    }

    /// The name a rename would act on from the cursor: a declaration's name,
    /// kind and file, or an identifier hit's token.
    pub fn rename_target(&self) -> Option<RenameTarget> {
        let m = self.current()?;
        if let Some(s) = &m.symbol {
            return Some(RenameTarget {
                name: s.name.clone(),
                symbol: Some(s.kind),
                declared_in: Some(m.path.clone()),
            });
        }
        let text = m.text.as_str();
        let mut chars = text.chars();
        let bare = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && chars.all(|c| c.is_alphanumeric() || c == '_');
        bare.then(|| RenameTarget {
            name: text.to_owned(),
            symbol: None,
            declared_in: None,
        })
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

// ---------------------------------------------------------------- shared

/// The status bar's message and whether the engine is busy.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Status {
    pub message: Option<(Level, String)>,
    pub busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

impl Status {
    pub fn info(&mut self, text: impl Into<String>) {
        self.message = Some((Level::Info, text.into()));
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.message = Some((Level::Error, text.into()));
    }

    pub fn clear(&mut self) {
        self.message = None;
    }
}

/// A file for a context or detail panel: its text, line boundaries, and
/// syntax colouring as byte spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePreview {
    pub path: RelPath,
    text: String,
    line_starts: Vec<usize>,
    pub highlights: Vec<Highlight>,
}

impl FilePreview {
    pub fn new(path: RelPath, text: String, highlights: Vec<Highlight>) -> Self {
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self {
            path,
            text,
            line_starts,
            highlights,
        }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Byte range of line `n`, terminator excluded.
    pub fn line_span(&self, n: usize) -> Option<(usize, usize)> {
        let start = *self.line_starts.get(n)?;
        let end = self
            .line_starts
            .get(n + 1)
            .map_or(self.text.len(), |&next| next);
        let end = self.text[start..end].trim_end_matches(['\n', '\r']).len() + start;
        Some((start, end))
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}
