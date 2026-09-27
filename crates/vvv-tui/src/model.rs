//! Application state, shared panel vocabulary, and retained mode selection.
//! Each mode owns its data and transitions under `crate::modes`.

use vvv_engine::RelPath;

use vvv_engine::Highlight;

use super::action::Action;
use super::keymap::{Dispatch, Layer, When};
use super::screen::Screen;
use crate::modes::history::HistoryMode;
use crate::modes::history::screen as history;
use crate::modes::moves::MoveMode;
use crate::modes::moves::screen as moving;
use crate::modes::rename::RenameMode;
use crate::modes::rename::screen as rename;
use crate::modes::rewrite::RewriteMode;
use crate::modes::rewrite::screen as rewrite;
use crate::modes::search::screen as search;
use crate::modes::search::{Search, SearchPanel};
pub(crate) use crate::overlays::{MenuTarget, Overlay};

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

    /// The selected report source, when the active overlay is a report.
    pub fn report_site(&self) -> Option<(RelPath, u32)> {
        self.overlay.as_ref()?.report_site()
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
        self.overlay.as_ref().map(Overlay::screen)
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
