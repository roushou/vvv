//! All TUI state as plain data: the search hub, the mode in front of it,
//! and an overlay when a small question is open. Nothing here does I/O or
//! knows about terminals; see `update.rs` for how it changes and `render/`
//! for how it looks.

use std::collections::BTreeSet;
use std::path::Path;

use vvv_engine::RelPath;

use vvv_engine::HistoryEntry;
use vvv_engine::protocol::FileChange;
use vvv_engine::report::{Detailed, Document, Options, View};
use vvv_engine::{
    Confidence, Highlight, Intent, Match, MatchId, Notice, Occurrence, Query, Respelling, Role,
    SymbolKind,
};

use super::action::Action;
use super::keymap::{Dispatch, Layer, When};
use super::query::QueryBar;
use super::screen::{Screen, history, moving, overlay, rename, rewrite, search};

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

/// Search results in the engine's order — declarations first — with the cursor.
#[derive(Debug, Default)]
pub struct Results {
    pub query: Option<Query>,
    pub matches: Vec<Match>,
    pub cursor: Cursor,
}

impl Results {
    pub fn replace(&mut self, matches: Vec<Match>) {
        self.matches = matches;
        self.cursor.clamp(self.matches.len());
    }

    pub fn current(&self) -> Option<&Match> {
        self.matches.get(self.cursor.index)
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

/// What `r` found under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTarget {
    pub name: String,
    pub symbol: Option<SymbolKind>,
    pub declared_in: Option<RelPath>,
}

// ---------------------------------------------------------------- rename

/// A rename being judged: the new name, every occurrence by verdict, and
/// which of them are ticked for the commit.
#[derive(Debug)]
pub struct RenameMode {
    pub target: RenameTarget,
    pub language: Option<vvv_engine::LanguageId>,
    /// The new name, edited live; the plan is re-made as it grows.
    pub name: String,
    pub declarations: Vec<Match>,
    pub occurrences: Vec<Occurrence>,
    /// The last plan's files, each holding its diff: the preview the detail
    /// pane draws.
    pub changes: Vec<FileChange>,
    pub ticks: BTreeSet<MatchId>,
    pub focus: RenamePanel,
    /// Cursors of the `?`, `✓` and `✗` panels, in that order.
    pub cursors: [Cursor; 3],
    /// The list panel the detail follows when focus is elsewhere.
    pub last_list: RenamePanel,
    pub detail_scroll: usize,
    pub preview: Option<FilePreview>,
    /// The verdicts are in and the ticks seeded; later plans only refresh
    /// `changes`, so typing does not undo the user's ticks.
    pub judged: bool,
    /// Waiting for the judge, or for the commit.
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenamePanel {
    Name,
    Unsure,
    Sure,
    Other,
    Detail,
}

impl Panels for RenamePanel {
    const ALL: &'static [Self] = &[
        Self::Name,
        Self::Unsure,
        Self::Sure,
        Self::Other,
        Self::Detail,
    ];
}

impl RenamePanel {
    pub fn confidence(self) -> Option<Confidence> {
        match self {
            Self::Unsure => Some(Confidence::Unresolved),
            Self::Sure => Some(Confidence::Resolved),
            Self::Other => Some(Confidence::Other),
            Self::Name | Self::Detail => None,
        }
    }

    fn slot(self) -> Option<usize> {
        match self {
            Self::Unsure => Some(0),
            Self::Sure => Some(1),
            Self::Other => Some(2),
            Self::Name | Self::Detail => None,
        }
    }
}

impl RenameMode {
    pub fn new(target: RenameTarget, language: Option<vvv_engine::LanguageId>) -> Self {
        Self {
            name: String::new(),
            target,
            language,
            declarations: Vec::new(),
            occurrences: Vec::new(),
            changes: Vec::new(),
            ticks: BTreeSet::new(),
            focus: RenamePanel::Name,
            cursors: [Cursor::default(); 3],
            last_list: RenamePanel::Unsure,
            detail_scroll: 0,
            preview: None,
            judged: false,
            busy: true,
            error: None,
        }
    }

    /// The rows of one verdict panel, in the engine's order.
    pub fn rows(&self, confidence: Confidence) -> Vec<&Occurrence> {
        self.occurrences
            .iter()
            .filter(|o| o.confidence == confidence)
            .collect()
    }

    pub fn cursor(&self, panel: RenamePanel) -> Option<&Cursor> {
        panel.slot().map(|i| &self.cursors[i])
    }

    pub fn cursor_mut(&mut self, panel: RenamePanel) -> Option<&mut Cursor> {
        panel.slot().map(move |i| &mut self.cursors[i])
    }

    /// The list panel whose row the detail explains.
    pub fn list(&self) -> RenamePanel {
        if self.focus.confidence().is_some() {
            self.focus
        } else {
            self.last_list
        }
    }

    pub fn current(&self) -> Option<&Occurrence> {
        let panel = self.list();
        let rows = self.rows(panel.confidence()?);
        rows.get(self.cursor(panel)?.index).copied()
    }

    pub fn is_ticked(&self, o: &Occurrence) -> bool {
        self.ticks.contains(&o.m.id)
    }

    pub fn ticked(&self, confidence: Confidence) -> usize {
        self.rows(confidence)
            .iter()
            .filter(|o| self.is_ticked(o))
            .count()
    }

    pub fn toggle(&mut self) {
        if let Some(id) = self.current().map(|o| o.m.id.clone())
            && !self.ticks.remove(&id)
        {
            self.ticks.insert(id);
        }
    }

    /// Tick every row of the focused panel, or untick them all when they
    /// already are.
    pub fn toggle_panel(&mut self) {
        let Some(confidence) = self.list().confidence() else {
            return;
        };
        let ids: Vec<MatchId> = self
            .rows(confidence)
            .iter()
            .map(|o| o.m.id.clone())
            .collect();
        if ids.iter().all(|id| self.ticks.contains(id)) {
            for id in &ids {
                self.ticks.remove(id);
            }
        } else {
            self.ticks.extend(ids);
        }
    }

    /// The plan's change for the occurrence's file when the plan edits this
    /// very site: the diff the detail pane draws. A file with only other
    /// sites changed is not this row's preview.
    pub fn file(&self, o: &Occurrence) -> Option<&FileChange> {
        self.changes
            .iter()
            .find(|f| f.path == o.m.path && f.edits.iter().any(|e| e.span == o.m.span))
    }

    /// Files the commit touches, from the ticks.
    pub fn files(&self) -> usize {
        self.occurrences
            .iter()
            .filter(|o| self.is_ticked(o))
            .map(|o| o.m.path.as_path())
            .collect::<BTreeSet<&Path>>()
            .len()
    }
}

// ---------------------------------------------------------------- move

/// A move being planned: the destination, edited live, and the plan it
/// yields — or why it yields none.
#[derive(Debug)]
pub struct MoveMode {
    pub from: RelPath,
    /// `Some` when one declaration moves rather than the file.
    pub symbol: Option<String>,
    pub to: String,
    pub plan: Option<MovePlan>,
    pub error: Option<String>,
    pub focus: MovePanel,
    /// Cursors of the `→`, `±` and `!` panels.
    pub cursors: [Cursor; 3],
    pub last_list: MovePanel,
    pub detail_scroll: usize,
    pub preview: Option<FilePreview>,
    /// Show the whole file diff in the detail panel.
    pub diff: bool,
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub struct MovePlan {
    pub intent: Intent,
    pub files: Vec<FileChange>,
    pub respellings: Vec<Respelling>,
    pub notices: Vec<Notice>,
    /// Indices into `files` of the structural changes: moved, or holding an
    /// edit no respelling accounts for.
    pub structural: Vec<usize>,
}

impl MovePlan {
    pub fn new(
        intent: Intent,
        files: Vec<FileChange>,
        respellings: Vec<Respelling>,
        notices: Vec<Notice>,
    ) -> Self {
        let structural = files
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                f.moved_to.is_some()
                    || f.edits.is_empty()
                    || !f.edits.iter().all(|e| {
                        respellings
                            .iter()
                            .any(|r| r.path == f.path && r.span == e.span)
                    })
            })
            .map(|(i, _)| i)
            .collect();
        Self {
            intent,
            files,
            respellings,
            notices,
            structural,
        }
    }

    /// One line per structural file: where it goes, or its first changed line.
    pub fn structural_label(&self, i: usize) -> String {
        let file = &self.files[i];
        if let Some(to) = &file.moved_to {
            return format!("{} → {}", file.path.short(), to.short());
        }
        let change = file
            .diff
            .as_str()
            .lines()
            .find(|l| {
                (l.starts_with('-') || l.starts_with('+'))
                    && !l.starts_with("---")
                    && !l.starts_with("+++")
            })
            .map(|l| format!("{} {}", &l[..1], l[1..].trim()))
            .unwrap_or_default();
        format!("{}  {change}", file.path.short())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovePanel {
    To,
    Respellings,
    Structural,
    Notices,
    Detail,
}

impl Panels for MovePanel {
    const ALL: &'static [Self] = &[
        Self::To,
        Self::Respellings,
        Self::Structural,
        Self::Notices,
        Self::Detail,
    ];
}

impl MovePanel {
    fn slot(self) -> Option<usize> {
        match self {
            Self::Respellings => Some(0),
            Self::Structural => Some(1),
            Self::Notices => Some(2),
            Self::To | Self::Detail => None,
        }
    }
}

/// What a move row points at, for the detail panel and the editor.
#[derive(Debug, Clone)]
pub enum MoveRow<'a> {
    Respelling(&'a Respelling),
    Structural(&'a FileChange),
    Notice(&'a Notice),
}

impl MoveRow<'_> {
    pub fn path(&self) -> &RelPath {
        match self {
            Self::Respelling(r) => &r.path,
            Self::Structural(f) => &f.path,
            Self::Notice(n) => &n.path,
        }
    }

    pub fn line(&self) -> u32 {
        match self {
            Self::Respelling(r) => r.start.line,
            Self::Structural(_) => 0,
            Self::Notice(n) => n.start.line,
        }
    }
}

impl MoveMode {
    pub fn new(from: RelPath, symbol: Option<String>) -> Self {
        let to = match &symbol {
            Some(_) => String::new(),
            None => from.short(),
        };
        Self {
            from,
            symbol,
            to,
            plan: None,
            error: None,
            focus: MovePanel::To,
            cursors: [Cursor::default(); 3],
            last_list: MovePanel::Respellings,
            detail_scroll: 0,
            preview: None,
            diff: false,
            busy: false,
        }
    }

    pub fn intent(&self) -> Option<Intent> {
        let to = self.to.trim();
        if to.is_empty() {
            return None;
        }
        Some(match &self.symbol {
            Some(name) => {
                Intent::MoveSymbol(vvv_engine::MoveSymbolIntent::new(name, &self.from, to))
            }
            None => Intent::Move(vvv_engine::MoveIntent::new(&self.from, to)),
        })
    }

    pub fn len(&self, panel: MovePanel) -> usize {
        let Some(plan) = &self.plan else {
            return 0;
        };
        match panel {
            MovePanel::Respellings => plan.respellings.len(),
            MovePanel::Structural => plan.structural.len(),
            MovePanel::Notices => plan.notices.len(),
            MovePanel::To | MovePanel::Detail => 0,
        }
    }

    pub fn cursor(&self, panel: MovePanel) -> Option<&Cursor> {
        panel.slot().map(|i| &self.cursors[i])
    }

    pub fn cursor_mut(&mut self, panel: MovePanel) -> Option<&mut Cursor> {
        panel.slot().map(move |i| &mut self.cursors[i])
    }

    pub fn list(&self) -> MovePanel {
        if self.focus.slot().is_some() {
            self.focus
        } else {
            self.last_list
        }
    }

    pub fn current(&self) -> Option<MoveRow<'_>> {
        let plan = self.plan.as_ref()?;
        let panel = self.list();
        let i = self.cursor(panel)?.index;
        Some(match panel {
            MovePanel::Respellings => MoveRow::Respelling(plan.respellings.get(i)?),
            MovePanel::Structural => MoveRow::Structural(&plan.files[*plan.structural.get(i)?]),
            MovePanel::Notices => MoveRow::Notice(plan.notices.get(i)?),
            MovePanel::To | MovePanel::Detail => return None,
        })
    }
}

// ---------------------------------------------------------------- rewrite

/// A rewrite being shaped: the search's matches, the template edited live,
/// and what each match becomes.
#[derive(Debug)]
pub struct RewriteMode {
    pub query: Query,
    pub template: String,
    pub matches: Vec<Match>,
    /// The last plan's files, each holding its diff: the preview the detail
    /// pane draws.
    pub changes: Vec<FileChange>,
    pub ticks: BTreeSet<MatchId>,
    pub focus: RewritePanel,
    pub cursor: Cursor,
    pub detail_scroll: usize,
    pub error: Option<String>,
    pub busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewritePanel {
    Template,
    Matches,
    Detail,
}

impl Panels for RewritePanel {
    const ALL: &'static [Self] = &[Self::Template, Self::Matches, Self::Detail];
}

impl RewriteMode {
    pub fn new(query: Query, matches: Vec<Match>) -> Self {
        let ticks = matches.iter().map(|m| m.id.clone()).collect();
        Self {
            query,
            template: String::new(),
            matches,
            changes: Vec::new(),
            ticks,
            focus: RewritePanel::Template,
            cursor: Cursor::default(),
            detail_scroll: 0,
            error: None,
            busy: false,
        }
    }

    pub fn intent(&self) -> Option<vvv_engine::RewriteIntent> {
        (!self.template.trim().is_empty())
            .then(|| vvv_engine::RewriteIntent::new(self.query.clone(), self.template.as_str()))
    }

    pub fn current(&self) -> Option<&Match> {
        self.matches.get(self.cursor.index)
    }

    pub fn is_ticked(&self, m: &Match) -> bool {
        self.ticks.contains(&m.id)
    }

    pub fn toggle(&mut self) {
        if let Some(id) = self.current().map(|m| m.id.clone())
            && !self.ticks.remove(&id)
        {
            self.ticks.insert(id);
        }
    }

    pub fn toggle_all(&mut self) {
        if self.ticks.len() == self.matches.len() {
            self.ticks.clear();
        } else {
            self.ticks = self.matches.iter().map(|m| m.id.clone()).collect();
        }
    }

    pub fn files(&self) -> usize {
        self.matches
            .iter()
            .filter(|m| self.is_ticked(m))
            .map(|m| m.path.as_path())
            .collect::<BTreeSet<&Path>>()
            .len()
    }
}

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

    pub fn title(&self) -> &'static str {
        match self.target {
            MenuTarget::Symbol => "symbol kind",
            MenuTarget::Language => "language",
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
