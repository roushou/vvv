//! Application state, shared panel vocabulary, and retained mode selection.
//! Each mode owns its data and transitions under `crate::modes`.

use vvv_engine::RelPath;

use crate::modes::context::ModeContext;
use crate::overlays::{Confirmed, Menu};
use ratatui::crossterm::event::KeyEvent;
use vvv_engine::Intent;
use vvv_engine::protocol::vocabulary::IntentLine;

use super::action::{Action, Effect, Event, Planned};
use super::keymap::{Dispatch, Key, Layer, When};
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
use crate::modes::search::{Navigation, Search, SearchPanel};
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
    /// One compact row per hit.
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

    /// Map a key to an action for the current view and focus. `None` =
    /// ignored.
    pub fn action_for(&self, event: KeyEvent) -> Option<Action> {
        let key = Key::from_event(event)?;
        match self
            .screen()
            .resolve(self.focus(), key, |when| self.holds(when))?
        {
            Dispatch::Run(action) => Some(action),
            Dispatch::Type => key.text().map(Action::Input),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match self.action_for(key) {
            Some(action) => self.update(action),
            None => Vec::new(),
        }
    }

    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Start => Vec::new(),
            Action::Quit => {
                self.quit = true;
                Vec::new()
            }
            Action::Move(n) => {
                if matches!(self.overlay, Some(Overlay::Report { .. })) {
                    if let Some(overlay) = &mut self.overlay {
                        overlay.report_moved(n);
                    }
                    Vec::new()
                } else {
                    self.moved(n)
                }
            }
            Action::Page(n) => self.moved(n * 10),
            Action::Top => self.jump(true),
            Action::Bottom => self.jump(false),
            Action::Scroll(n) => self.scrolled(n),
            Action::Resize(by) => {
                self.split = (i32::from(self.split) + i32::from(by)).clamp(20, 80) as u16;
                Vec::new()
            }
            Action::View => {
                self.view = self.view.toggled();
                Vec::new()
            }
            Action::Enter => self.entered(),
            Action::Back => self.back(),
            Action::Rename => self.enter_rename(),
            Action::MoveFile => self.enter_move(false),
            Action::MoveSymbol => self.enter_move(true),
            Action::Rewrite => self.enter_rewrite(),
            Action::History => {
                self.status.busy = true;
                vec![Effect::History]
            }
            Action::OpenMenu(target) => self.open_menu(target),
            Action::MenuChoose => self.choose_menu(),
            Action::Undo => self.undo_requested(),
            Action::Help => {
                self.overlay = Some(Overlay::Help {
                    screen: self.mode_screen(),
                    focus: self.focus(),
                    scroll: 0,
                });
                Vec::new()
            }
            Action::Edit => self.edit(),
            Action::Jump => self.goto_declaration(),
            _ => self.mode_update(action),
        }
    }
    fn mode_update(&mut self, action: Action) -> Vec<Effect> {
        let mut context = ModeContext {
            status: &mut self.status,
            generation: &mut self.generation,
        };
        match &mut self.mode {
            Mode::Search => self.search.update(action, &mut context),
            Mode::Rename(r) => r.update(action, &mut context),
            Mode::Move(mv) => mv.update(action, &mut context),
            Mode::Rewrite(rw) => rw.update(action, &mut context),
            Mode::History(h) => h.update(action, &mut context),
        }
    }

    pub fn on_event(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::DefinitionResolved {
                revision,
                query,
                references,
            } => {
                if !self
                    .search
                    .results
                    .definition_resolved(revision, query, references)
                {
                    return Vec::new();
                }
                self.search.selection_changed();
                self.preview_effect()
            }
            Event::Searched {
                generation,
                matches,
                skipped,
            } => {
                if generation != self.generation {
                    return Vec::new();
                }
                self.search.searched(
                    matches,
                    skipped,
                    &mut ModeContext {
                        status: &mut self.status,
                        generation: &mut self.generation,
                    },
                );
                self.preview_effect()
            }
            Event::Answered { generation, answer } => {
                if generation != self.generation {
                    return Vec::new();
                }
                if !self.search.answered(
                    *answer,
                    &mut ModeContext {
                        status: &mut self.status,
                        generation: &mut self.generation,
                    },
                ) {
                    return Vec::new();
                }
                self.preview_effect()
            }
            Event::Previewed {
                path,
                text,
                highlights,
                symbols,
            } => {
                let preview = FilePreview::new(vvv_engine::File {
                    path,
                    text,
                    highlights,
                    symbols,
                });
                match &mut self.mode {
                    Mode::Search => self.search.previewed(preview),
                    Mode::Rename(r) => r.previewed(preview),
                    Mode::Move(mv) => mv.previewed(preview),
                    // Rewrite's pane draws the plan's diff, not a source file.
                    Mode::Rewrite(_) => {}
                    Mode::History(_) => {}
                }
                Vec::new()
            }
            Event::Planned {
                generation,
                planned,
            } => {
                if generation != self.generation {
                    return Vec::new();
                }
                self.arriving = false;
                self.planned(planned)
            }
            Event::PlanFailed {
                generation,
                message,
            } => {
                if generation != self.generation {
                    return Vec::new();
                }
                self.arriving = false;
                match &mut self.mode {
                    Mode::Rename(r) => r.plan_failed(message),
                    Mode::Move(mv) => mv.plan_failed(message),
                    Mode::Rewrite(rw) => rw.plan_failed(message),
                    Mode::Search | Mode::History(_) => self.status.error(message),
                }
                Vec::new()
            }
            Event::Applied { id, intent, report } => {
                self.mode = Mode::Search;
                self.search.preview = None;
                self.search.body.clear();
                self.overlay = Some(Overlay::Report {
                    report: Box::new(report),
                    cursor: 0,
                });
                let effects = self.search();
                self.status.busy = false;
                self.status
                    .info(format!("✓ #{id}  {}  ·  u undoes it", IntentLine(&intent)));
                effects
            }
            Event::History(entries) => {
                self.status.busy = false;
                if entries.is_empty() {
                    self.status.info("∅ no history");
                } else {
                    self.mode = Mode::History(HistoryMode::new(entries));
                }
                Vec::new()
            }
            Event::Undone(entry) => {
                self.mode = Mode::Search;
                self.search.preview = None;
                self.search.body.clear();
                let effects = self.search();
                self.status.busy = false;
                self.status
                    .info(format!("↩ #{}  {}", entry.id, IntentLine(&entry.intent)));
                effects
            }
            Event::Failed(message) => {
                self.status.busy = false;
                self.arriving = false;
                match &mut self.mode {
                    Mode::Rename(r) => r.failed(),
                    Mode::Move(mv) => mv.failed(),
                    Mode::Rewrite(rw) => rw.failed(),
                    Mode::Search | Mode::History(_) => {}
                }
                self.status.error(message);
                Vec::new()
            }
        }
    }

    /// A plan answered: the mode that asked takes what it shows.
    fn planned(&mut self, planned: Planned) -> Vec<Effect> {
        match (&mut self.mode, planned) {
            (
                Mode::Rename(r),
                Planned::Rename {
                    declarations,
                    occurrences,
                    files,
                    ..
                },
            ) => r.planned(declarations, occurrences, files),
            (
                Mode::Move(mv),
                Planned::Move {
                    intent,
                    respellings,
                    notices,
                    files,
                },
            ) => mv.planned(intent, respellings, notices, files),
            (Mode::Rewrite(rw), Planned::Rewrite { files }) => rw.planned(files),
            _ => Vec::new(),
        }
    }

    // ------------------------------------------------------------ cursors

    fn moved(&mut self, by: i32) -> Vec<Effect> {
        if let Some(Overlay::Menu(menu)) = &mut self.overlay {
            menu.move_cursor(by);
            Vec::new()
        } else {
            self.mode_update(Action::Move(by))
        }
    }

    fn jump(&mut self, top: bool) -> Vec<Effect> {
        if matches!(self.mode, Mode::Search) && self.search.scroll_focused() {
            self.search.jump(top)
        } else {
            self.moved(if top { i32::MIN / 2 } else { i32::MAX / 2 })
        }
    }

    /// Whether the focused panel is a text panel that scrolls rather than
    /// a list with a cursor.
    fn scroll_focused(&self) -> bool {
        match &self.mode {
            Mode::Search => self.search.scroll_focused(),
            Mode::Rename(r) => r.scroll_focused(),
            Mode::Move(mv) => mv.scroll_focused(),
            Mode::Rewrite(rw) => rw.scroll_focused(),
            Mode::History(h) => h.scroll_focused(),
        }
    }

    fn scrolled(&mut self, by: i32) -> Vec<Effect> {
        if self
            .overlay
            .as_mut()
            .is_some_and(|overlay| overlay.help_scrolled(by))
        {
            Vec::new()
        } else if !self.scroll_focused() {
            self.moved(by)
        } else {
            self.mode_update(Action::Scroll(by))
        }
    }

    /// The file the focused row is in, if the mode's detail does not show
    /// it yet.
    fn preview_effect(&self) -> Vec<Effect> {
        match &self.mode {
            Mode::Search => self.search.preview_effect(),
            Mode::Rename(r) => r.preview_effect(),
            Mode::Move(mv) => mv.preview_effect(),
            Mode::Rewrite(_) | Mode::History(_) => Vec::new(),
        }
    }

    // ------------------------------------------------------------ inputs

    fn search(&mut self) -> Vec<Effect> {
        self.search.search(&mut ModeContext {
            status: &mut self.status,
            generation: &mut self.generation,
        })
    }

    fn plan_move(&mut self, debounce: bool) -> Vec<Effect> {
        let generation = self.next_generation();
        match &mut self.mode {
            Mode::Move(mv) => mv.plan(generation, debounce),
            _ => Vec::new(),
        }
    }

    // ------------------------------------------------------------ enter / back

    /// `⏎`: in search, go to the results; in a mode, commit; on a
    /// confirmation, yes.
    fn entered(&mut self) -> Vec<Effect> {
        if let Some(Overlay::Confirm(c)) = &self.overlay {
            let then = c.then.clone();
            self.overlay = None;
            self.status.busy = true;
            return match then {
                Confirmed::Undo => vec![Effect::Undo],
            };
        }
        if matches!(self.mode, Mode::History(_)) {
            self.undo_requested()
        } else {
            self.mode_update(Action::Enter)
        }
    }

    fn back(&mut self) -> Vec<Effect> {
        if self.overlay.take().is_some() {
            return Vec::new();
        }
        if !matches!(self.mode, Mode::Search) {
            self.mode = Mode::Search;
            self.arriving = false;
            self.status.clear();
            return self.preview_effect();
        }
        self.search.back(&mut ModeContext {
            status: &mut self.status,
            generation: &mut self.generation,
        })
    }

    // ------------------------------------------------------------ modes

    /// A retained-hub selection follows the active mode's preview.
    fn navigation(&mut self, navigation: Navigation) -> Vec<Effect> {
        match navigation {
            Navigation::Selection => {
                self.search.selection_changed();
                self.preview_effect()
            }
            Navigation::Effects(effects) => effects,
        }
    }

    fn goto_declaration(&mut self) -> Vec<Effect> {
        let navigation = self.search.goto_declaration(&mut ModeContext {
            status: &mut self.status,
            generation: &mut self.generation,
        });
        self.navigation(navigation)
    }

    fn enter_rename(&mut self) -> Vec<Effect> {
        let r = match RenameMode::from_results(&self.search.results) {
            Ok(r) => r,
            Err(message) => return self.fail(message),
        };
        let intent = r.judgment();
        self.mode = Mode::Rename(Box::new(r));
        self.arriving = true;
        let generation = self.next_generation();
        vec![Effect::Plan {
            generation,
            intent: Intent::Rename(intent),
            debounce: false,
        }]
    }

    fn enter_move(&mut self, symbol: bool) -> Vec<Effect> {
        let mv = match MoveMode::from_results(&self.search.results, symbol) {
            Ok(mv) => mv,
            Err(message) => return self.fail(message),
        };
        self.mode = Mode::Move(Box::new(mv));
        let effects = self.plan_move(false);
        self.arriving = !effects.is_empty();
        effects
    }

    fn enter_rewrite(&mut self) -> Vec<Effect> {
        let rw = match RewriteMode::from_results(&self.search.results) {
            Ok(rw) => rw,
            Err(message) => return self.fail(message),
        };
        self.mode = Mode::Rewrite(Box::new(rw));
        self.preview_effect()
    }

    fn open_menu(&mut self, target: MenuTarget) -> Vec<Effect> {
        self.overlay = Some(Overlay::Menu(Menu::for_target(
            target,
            &self.languages,
            &self.search,
        )));
        Vec::new()
    }

    fn choose_menu(&mut self) -> Vec<Effect> {
        let Some(Overlay::Menu(menu)) = &self.overlay else {
            return Vec::new();
        };
        let choice = menu.chosen();
        self.overlay = None;
        let navigation = self.search.choose_menu(
            choice,
            &mut ModeContext {
                status: &mut self.status,
                generation: &mut self.generation,
            },
        );
        self.navigation(navigation)
    }

    fn undo_requested(&mut self) -> Vec<Effect> {
        match &self.mode {
            Mode::History(h) => match h.confirmation() {
                Ok(confirm) => {
                    self.overlay = Some(Overlay::Confirm(confirm));
                    Vec::new()
                }
                Err(message) => self.fail(message),
            },
            _ => {
                self.status.busy = true;
                vec![Effect::History]
            }
        }
    }

    /// `$EDITOR` at the focused row's line.
    fn edit(&mut self) -> Vec<Effect> {
        if matches!(self.overlay, Some(Overlay::Report { .. })) {
            return match self.report_site() {
                Some((path, line)) => vec![Effect::Edit { path, line }],
                None => Vec::new(),
            };
        }
        let site = match &self.mode {
            Mode::Search => self.search.site(),
            Mode::Rename(r) => r.site(),
            Mode::Move(mv) => mv.site(),
            Mode::Rewrite(rw) => rw.site(),
            Mode::History(_) => None,
        };
        match site {
            Some((path, line)) => vec![Effect::Edit { path, line }],
            None => Vec::new(),
        }
    }

    fn fail(&mut self, message: &str) -> Vec<Effect> {
        self.status.error(message);
        Vec::new()
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
    file: vvv_engine::File,
    line_starts: Vec<usize>,
}

impl FilePreview {
    pub fn new(file: vvv_engine::File) -> Self {
        let line_starts = std::iter::once(0)
            .chain(file.text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self { file, line_starts }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Lines intersecting a nonempty, valid UTF-8 source range.
    pub fn lines_in(&self, span: vvv_engine::Span) -> Option<std::ops::Range<usize>> {
        if span.is_empty() || self.text.get(span.start..span.end).is_none() {
            return None;
        }
        let start = self.line_starts.partition_point(|&i| i <= span.start) - 1;
        let end = self.line_starts.partition_point(|&i| i < span.end);
        Some(start..end)
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
        &self.file.text
    }
}

impl std::ops::Deref for FilePreview {
    type Target = vvv_engine::File;

    fn deref(&self) -> &Self::Target {
        &self.file
    }
}
