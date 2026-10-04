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
    viewport: Option<(u16, u16)>,
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
            When::Recoverable => {
                self.overlay.is_none()
                    && !self.status.busy
                    && !self.mode.busy()
                    && self
                        .problem()
                        .is_some_and(crate::problem::Problem::can_retry)
            }
            When::Problem => {
                !matches!(self.mode, Mode::History(_))
                    && self.overlay.is_none()
                    && !self.input_focused()
                    && self.problem().is_some_and(|p| p.failure.recovery.is_none())
            }
            When::RecoveryFile => {
                self.overlay.is_none()
                    && !self.input_focused()
                    && self.problem().is_some_and(|p| p.site().is_some())
            }
            When::ReportViewAvailable => {
                !matches!(self.mode, Mode::Search) || self.search.workspace.is_none()
            }
            When::InputFocused => self.input_focused(),
            When::PlacesRecent => matches!(&self.overlay, Some(Overlay::Places(p)) if p.recent),
            When::BrowseBack => self.search.trail.can_travel(false),
            When::BrowseForward => self.search.trail.can_travel(true),
            When::QueryEmpty => self.search.query.is_empty(),
            When::QueryNotEmpty => !self.search.query.is_empty(),
            When::SearchList => {
                matches!(self.search.focus, SearchPanel::Files | SearchPanel::Results)
                    && !self.search.input_focused()
            }
            When::SearchListAnchored => {
                self.search.results.is_anchored() && self.holds(When::SearchList)
            }
            When::FileList => self.search.results.has_file_list() && !self.search.input_focused(),
        }
    }

    /// The selected report source, when the active overlay is a report.
    pub fn report_site(&self) -> Option<(RelPath, u32)> {
        self.overlay.as_ref()?.report_site()
    }

    /// The view of the mode on screen.
    pub fn mode_screen(&self) -> &'static Screen {
        match self.shown() {
            Mode::Search => self.search.screen(),
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
            (Action::MenuChoose, _) => if matches!(&self.overlay, Some(Overlay::Menu(menu)) if menu.target == MenuTarget::Filters) {
                "edit"
            } else {
                "choose"
            }.to_owned(),
            (Action::Recover, Mode::Search) => "refresh".into(),
            (Action::Recover, _) => "rebuild preview".into(),
            (Action::Enter, Mode::Rename(r)) => {
                if r.state() == crate::modes::review::ReviewState::Ready {
                    format!("apply {} in {}", r.ticks.len(), files(r.files()))
                } else {
                    r.state().apply_hint().to_owned()
                }
            }
            (Action::Enter, Mode::Rewrite(rw)) => {
                if rw.state() == crate::modes::review::ReviewState::Ready {
                    format!("apply {} in {}", rw.ticks.len(), files(rw.files()))
                } else {
                    rw.state().apply_hint().to_owned()
                }
            }
            (Action::Enter, Mode::Move(mv)) => {
                if mv.state() == crate::modes::review::ReviewState::Ready {
                    format!(
                        "apply {}",
                        files(mv.plan.as_ref().map_or(0, |p| p.files.len()))
                    )
                } else {
                    mv.state().apply_hint().to_owned()
                }
            }
            (Action::ExpandPreview, Mode::Search) => if self.search.expanded.is_some() {
                "restore"
            } else {
                "expand"
            }
            .to_owned(),
            (Action::Back, Mode::Search) if self.search.workspace.is_some() => "search".into(),
            (Action::Back, Mode::Search) => if self.search.expanded.is_some() {
                "restore"
            } else {
                "results"
            }
            .to_owned(),
            (Action::Enter, _) => "apply".to_owned(),
            (Action::Undo, Mode::History(h)) => {
                if h.is_newest() {
                    h.current()
                        .map_or("nothing to undo".to_owned(), |e| format!("undo #{}", e.id))
                } else {
                    "newest entry only".to_owned()
                }
            }
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
            viewport: None,
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

    pub fn problem(&self) -> Option<&crate::problem::Problem> {
        match &self.mode {
            Mode::Search => self
                .search
                .problem
                .as_ref()
                .or(self.search.body.problem.as_ref()),
            Mode::Rename(r) => r.error.as_ref(),
            Mode::Move(m) => m.error.as_ref(),
            Mode::Rewrite(r) => r.error.as_ref(),
            Mode::History(_) => self.status.problem.as_ref(),
        }
    }
    fn problem_mut(&mut self) -> Option<&mut crate::problem::Problem> {
        match &mut self.mode {
            Mode::Search => {
                if self.search.problem.is_some() {
                    self.search.problem.as_mut()
                } else {
                    self.search.body.problem.as_mut()
                }
            }
            Mode::Rename(r) => r.error.as_mut(),
            Mode::Move(m) => m.error.as_mut(),
            Mode::Rewrite(r) => r.error.as_mut(),
            Mode::History(_) => self.status.problem.as_mut(),
        }
    }
    fn recover(&mut self) -> Vec<Effect> {
        if self.status.busy || self.mode.busy() {
            return Vec::new();
        }
        let Some(problem) = self.problem().filter(|p| p.can_retry()) else {
            return Vec::new();
        };
        let retry = problem.retry.clone();
        self.status.clear();
        let generation = self.next_generation();
        let mut context = ModeContext {
            status: &mut self.status,
            generation: &mut self.generation,
        };
        match &mut self.mode {
            Mode::Search => {
                self.search.problem = None;
                if matches!(retry, Some(Effect::History | Effect::Undo)) {
                    context.status.busy = true;
                    return retry.into_iter().collect();
                }
                self.search.update(Action::Refresh, &mut context)
            }
            Mode::Rename(r) => r.plan(generation, false),
            Mode::Move(m) => m.plan(generation, false),
            Mode::Rewrite(r) => r.plan(generation),
            Mode::History(_) => {
                self.status.busy = true;
                retry.into_iter().collect()
            }
        }
    }
    pub fn input_focused(&self) -> bool {
        if let Some(overlay) = &self.overlay {
            return matches!(
                overlay,
                Overlay::Menu(_) | Overlay::Places(_) | Overlay::Navigation(_)
            );
        }
        match &self.mode {
            Mode::Search => self.search.input_focused(),
            Mode::Rename(r) => r.focus == crate::modes::rename::RenamePanel::Name,
            Mode::Move(m) => m.focus == crate::modes::moves::MovePanel::To,
            Mode::Rewrite(r) => r.focus == crate::modes::rewrite::RewritePanel::Template,
            Mode::History(_) => false,
        }
    }
    fn edit_input(&mut self, edit: crate::input::Edit<'_>) -> Vec<Effect> {
        if let Some(overlay) = &mut self.overlay {
            overlay.edit_input(edit);
            return Vec::new();
        }
        let mut context = ModeContext {
            status: &mut self.status,
            generation: &mut self.generation,
        };
        let effects = match &mut self.mode {
            Mode::Search => self.search.edit_input(edit, &mut context),
            Mode::Rename(r) => r.edit_input(edit, &mut context),
            Mode::Move(m) => m.edit_input(edit, &mut context),
            Mode::Rewrite(r) => r.edit_input(edit, &mut context),
            Mode::History(_) => Vec::new(),
        };
        self.sync_viewport();
        effects
    }
    pub fn paste(&mut self, text: &str) -> Vec<Effect> {
        if !self.input_focused() {
            return Vec::new();
        }
        let normalized = text.replace("\r\n", "\n");
        let text = if self.overlay.is_none() && matches!(self.mode, Mode::Rewrite(_)) {
            normalized.replace('\r', "\n")
        } else {
            normalized
                .trim_end_matches(['\r', '\n'])
                .replace(['\r', '\n'], " ")
        };
        self.edit_input(crate::input::Edit::Insert(&text))
    }
    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        if self.overlay.is_none()
            && self.scroll_focused()
            && matches!(
                action,
                Action::Scroll(_) | Action::Top | Action::Bottom | Action::Page(_)
            )
            && let Some(problem) = self.problem_mut()
        {
            problem.navigate(action);
            return Vec::new();
        }
        if matches!(action, Action::Refresh | Action::Recover)
            && self.problem().is_some_and(|p| p.failure.recovery.is_some())
        {
            return Vec::new();
        }
        if self.input_focused() {
            let text;
            let edit = match action {
                Action::Input(c) => {
                    text = c.to_string();
                    Some(crate::input::Edit::Insert(&text))
                }
                Action::Backspace => Some(crate::input::Edit::Command(
                    crate::input::EditCommand::Backspace,
                )),
                Action::Clear => Some(crate::input::Edit::Clear),
                Action::InputEdit(command) => Some(crate::input::Edit::Command(command)),
                _ => None,
            };
            if let Some(edit) = edit {
                return self.edit_input(edit);
            }
        }

        if let Some(Overlay::Places(picker)) = &mut self.overlay {
            match action {
                Action::Input(_) | Action::Backspace | Action::Clear => {
                    picker.input(action);
                    return Vec::new();
                }
                Action::PlacesTab => {
                    picker.tab();
                    return Vec::new();
                }
                Action::ForgetSearch => {
                    if let Some(crate::overlays::places::Place::Recent(recipe)) = picker.chosen() {
                        self.search.recent.forget(&recipe);
                        picker.forget(&recipe);
                    }
                    return Vec::new();
                }
                Action::ResetLayout => {
                    self.split = 50;
                    self.view = ReportView::Compact;
                    self.search.definition_tab = false;
                    self.search.expanded = None;
                    self.sync_viewport();
                    self.status.info("Layout reset");
                    return Vec::new();
                }
                _ => {}
            }
        }
        if let Some(Overlay::Menu(menu)) = &mut self.overlay {
            match action {
                Action::Input(c) => {
                    menu.input(Some(c));
                    return Vec::new();
                }
                Action::Backspace => {
                    menu.input(None);
                    return Vec::new();
                }
                Action::Clear => {
                    menu.filter.clear();
                    menu.cursor = 0;
                    return Vec::new();
                }
                _ => {}
            }
        }
        if let Some(Overlay::Navigation(picker)) = &mut self.overlay {
            match action {
                Action::Input(c) => {
                    picker.input(Some(c));
                    return Vec::new();
                }
                Action::Backspace => {
                    picker.input(None);
                    return Vec::new();
                }
                _ => {}
            }
        }
        if matches!(
            action,
            Action::Help
                | Action::OpenMenu(_)
                | Action::Back
                | Action::Rename
                | Action::MoveFile
                | Action::MoveSymbol
                | Action::Rewrite
                | Action::History
                | Action::Edit
                | Action::Refresh
                | Action::Input(_)
                | Action::Backspace
                | Action::Clear
                | Action::Enter
        ) {
            self.search.trail.cancel();
        }
        match action {
            Action::Workspace if self.overlay.is_none() && matches!(self.mode, Mode::Search) => {
                self.search.workspace(&mut ModeContext {
                    status: &mut self.status,
                    generation: &mut self.generation,
                })
            }
            Action::Recover => self.recover(),
            Action::Places => {
                if !matches!(self.mode, Mode::Search) {
                    return Vec::new();
                }
                if !self.status.busy
                    && let Some(recipe) =
                        crate::modes::search::recall::SearchRecipe::capture(&self.search)
                {
                    self.search.recent.remember(recipe);
                }
                self.search.trail.cancel();
                self.overlay = Some(Overlay::Places(crate::overlays::places::Places::new(
                    &self.search,
                )));
                Vec::new()
            }
            Action::Follow => self.follow(),
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
            Action::Page(n) => {
                if matches!(self.overlay, Some(Overlay::Help { .. })) {
                    return self.scrolled(n.saturating_mul(10));
                }
                let page = if self.overlay.is_none()
                    && matches!(self.mode, Mode::Search)
                    && matches!(self.search.focus, SearchPanel::Files | SearchPanel::Results)
                {
                    self.search_page_size()
                } else {
                    10
                };
                self.moved(n * page as i32)
            }
            Action::Top => self.jump(true),
            Action::Bottom => self.jump(false),
            Action::Scroll(n) => self.scrolled(n),
            Action::Resize(by) => {
                self.split = (i32::from(self.split) + i32::from(by)).clamp(20, 80) as u16;
                self.sync_viewport();
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
            Action::MenuClear => {
                let Some(Overlay::Menu(menu)) = &self.overlay else {
                    return Vec::new();
                };
                if menu.target == MenuTarget::Filters {
                    let Some(choice) = menu.chosen() else {
                        return Vec::new();
                    };
                    self.overlay = None;
                    let navigation = self.search.choose_menu(
                        choice,
                        &mut ModeContext {
                            status: &mut self.status,
                            generation: &mut self.generation,
                        },
                    );
                    return self.navigation(navigation);
                }
                let target = menu.target;
                self.overlay = None;
                let navigation = self.search.choose_menu(
                    (target, None),
                    &mut ModeContext {
                        status: &mut self.status,
                        generation: &mut self.generation,
                    },
                );
                self.navigation(navigation)
            }
            Action::Undo => self.undo_requested(),
            Action::Help => {
                if matches!(self.overlay, Some(Overlay::Help { .. })) {
                    return self.back();
                }
                let screen = self.screen();
                let title = if self.overlay.is_none()
                    && matches!(self.shown(), Mode::Search)
                    && self.search.workspace.is_none()
                {
                    format!("Search · {}", self.search.focus.label())
                } else {
                    screen
                        .panel(self.focus())
                        .map_or(screen.layer.name, |p| p.layer.name)
                        .to_owned()
                };
                let sections = screen.sections(self.focus(), |when| self.holds(when));
                let previous = self.overlay.take().map(Box::new);
                self.overlay = Some(Overlay::Help {
                    title,
                    previous,
                    sections,
                    scroll: 0,
                });
                Vec::new()
            }
            Action::Edit => self.edit(),
            _ => self.mode_update(action),
        }
    }
    fn mode_update(&mut self, action: Action) -> Vec<Effect> {
        let mut context = ModeContext {
            status: &mut self.status,
            generation: &mut self.generation,
        };
        let effects = match &mut self.mode {
            Mode::Search => self.search.update(action, &mut context),
            Mode::Rename(r) => r.update(action, &mut context),
            Mode::Move(mv) => mv.update(action, &mut context),
            Mode::Rewrite(rw) => rw.update(action, &mut context),
            Mode::History(h) => h.update(action, &mut context),
        };
        self.sync_viewport();
        effects
    }

    pub(crate) fn search_frame(&self) -> crate::modes::search::files::SearchFrame {
        let Some((width, height)) = self.viewport else {
            return Default::default();
        };
        if !matches!(self.mode, Mode::Search)
            || self.overlay.is_some()
            || self.search.workspace.is_some()
        {
            return Default::default();
        }
        crate::modes::search::screen::SearchView::new(
            &self.search,
            &self.root,
            self.status.busy,
            crate::render::Painter::plain(),
            self.split,
            self.view,
        )
        .frame(ratatui::layout::Rect::new(
            0,
            0,
            width,
            height.saturating_sub(1),
        ))
    }

    fn search_page_size(&self) -> usize {
        let frame = self.search_frame();
        let Some(list) = frame
            .lists
            .iter()
            .find(|list| list.panel == self.search.focus)
        else {
            return 10;
        };
        if list.panel == SearchPanel::Files {
            let mut paths = std::collections::BTreeSet::new();
            for row in list
                .rows
                .iter()
                .skip(list.offset)
                .take(list.content.height as usize)
            {
                if let crate::modes::search::files::PointerIntent::File(path) = row {
                    paths.insert(path);
                }
            }
            paths.len().max(1)
        } else {
            (list.content.height as usize).max(1)
        }
    }

    fn sync_viewport(&mut self) {
        if self.search.workspace.is_some() {
            return;
        }
        let Some((width, height)) = self.viewport else {
            return;
        };
        let view = crate::modes::search::screen::SearchView::new(
            &self.search,
            &self.root,
            self.status.busy,
            crate::render::Painter::plain(),
            self.split,
            self.view,
        );
        let frame = view.frame(ratatui::layout::Rect::new(
            0,
            0,
            width,
            height.saturating_sub(1),
        ));
        let definition_rows = view.definition_rows(ratatui::layout::Rect::new(
            0,
            0,
            width,
            height.saturating_sub(1),
        ));
        self.search.body.viewport = Some(definition_rows);
        for (panel, area) in &frame.panels {
            if !area.is_empty() {
                match panel {
                    SearchPanel::Context => {
                        self.search.inspection.viewport =
                            Some(area.width.saturating_sub(10) as usize)
                    }
                    SearchPanel::Body => {
                        self.search.body.inspection.viewport =
                            Some(area.width.saturating_sub(3) as usize)
                    }
                    _ => {}
                }
            }
        }
        for list in frame.lists {
            if list.area.is_empty() {
                continue;
            }
            let viewport = if list.panel == SearchPanel::Files {
                &mut self.search.results.files.viewport
            } else if let Some(path) = self.search.results.current().map(|m| m.path.clone()) {
                self.search
                    .results
                    .files
                    .match_viewports
                    .entry(path)
                    .or_default()
            } else {
                continue;
            };
            viewport.offset = list.offset;
            viewport.reveal = false;
        }
    }

    pub fn on_event(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::WorkspaceFiles { generation, paths } => {
                if generation != self.generation || !matches!(self.mode, Mode::Search) {
                    return Vec::new();
                }
                let Some(workspace) = &mut self.search.workspace else {
                    return Vec::new();
                };
                self.status.busy = false;
                self.search.problem = None;
                workspace.install(paths)
            }

            Event::Paste(text) => self.paste(&text),
            Event::Pointer(pointer) => {
                if self.overlay.is_some() || !matches!(self.mode, Mode::Search) {
                    return Vec::new();
                }
                let mut context = ModeContext {
                    status: &mut self.status,
                    generation: &mut self.generation,
                };
                let effects = self.search.pointer(pointer, &mut context);
                self.sync_viewport();
                effects
            }
            Event::SourcesChanged => {
                if let Some(workspace) = &mut self.search.workspace {
                    workspace.loading = true;
                }
                self.search.preview_dirty = true;
                self.next_generation();
                self.search.trail.cancel();
                self.search.body.reticket(self.search.body.next_ticket());
                self.search.stale = true;
                self.status.busy = false;
                self.overlay = None;
                self.status
                    .info("Source may have changed; refresh with ctrl+r");
                Vec::new()
            }
            Event::Followed {
                ticket,
                query,
                reply,
            } => {
                if self
                    .search
                    .trail
                    .pending
                    .as_ref()
                    .is_some_and(|p| p.ticket == ticket && p.query == query && !p.restoring)
                    && let Ok(vvv_engine::NavigationReply {
                        outcome: vvv_engine::NavigationOutcome::Resolved { preview, .. },
                        ..
                    }) = &reply
                    && let Some((width, height)) = self.viewport
                {
                    let view = crate::modes::search::screen::SearchView::new(
                        &self.search,
                        &self.root,
                        self.status.busy,
                        crate::render::Painter::plain(),
                        self.split,
                        self.view,
                    );
                    self.search.body.viewport = Some(view.full_definition_rows(
                        ratatui::layout::Rect::new(0, 0, width, height.saturating_sub(1)),
                        &preview.declaration,
                    ));
                }
                let picker = self.search.followed(
                    ticket,
                    query,
                    reply,
                    &mut ModeContext {
                        status: &mut self.status,
                        generation: &mut self.generation,
                    },
                );
                if let Some(picker) = picker {
                    self.overlay = Some(Overlay::Navigation(picker));
                }
                self.sync_viewport();
                Vec::new()
            }
            Event::Viewport { width, height } => {
                self.viewport = Some((width, height));
                if let Some(overlay) = &mut self.overlay {
                    overlay.help_scrolled(0, (width, height));
                }
                self.sync_viewport();
                Vec::new()
            }
            Event::DefinitionResolved {
                ticket,
                query,
                reply,
            } => {
                self.sync_viewport();
                if let Ok(vvv_engine::NavigationReply {
                    outcome: vvv_engine::NavigationOutcome::Resolved { preview, .. },
                    ..
                }) = &reply
                    && let Some((width, height)) = self.viewport
                {
                    let view = crate::modes::search::screen::SearchView::new(
                        &self.search,
                        &self.root,
                        self.status.busy,
                        crate::render::Painter::plain(),
                        self.split,
                        self.view,
                    );
                    self.search.body.viewport = Some(view.definition_rows_for(
                        ratatui::layout::Rect::new(0, 0, width, height.saturating_sub(1)),
                        Some(&preview.declaration),
                    ));
                }
                self.search.body.resolved(ticket, &query, reply);
                self.sync_viewport();
                Vec::new()
            }
            Event::Searched {
                generation,
                matches,
                skipped,
            } => {
                if generation != self.generation {
                    return Vec::new();
                }
                self.search.problem = None;
                self.search.searched(
                    matches,
                    skipped,
                    &mut ModeContext {
                        status: &mut self.status,
                        generation: &mut self.generation,
                    },
                );
                self.sync_viewport();
                self.preview_effect()
            }
            Event::Answered { generation, answer } => {
                if generation != self.generation {
                    return Vec::new();
                }
                self.search.problem = None;
                if !self.search.answered(
                    *answer,
                    &mut ModeContext {
                        status: &mut self.status,
                        generation: &mut self.generation,
                    },
                ) {
                    return Vec::new();
                }
                self.sync_viewport();
                self.preview_effect()
            }
            Event::Previewed {
                path,
                text,
                highlights,
                symbols,
                identifiers,
            } => {
                let preview = FilePreview::new(vvv_engine::File {
                    path,
                    text,
                    highlights,
                    symbols,
                    identifiers,
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
                problem,
            } => {
                if generation != self.generation {
                    return Vec::new();
                }
                self.arriving = false;
                match &mut self.mode {
                    Mode::Rename(r) => r.plan_failed(*problem),
                    Mode::Move(mv) => mv.plan_failed(*problem),
                    Mode::Rewrite(rw) => rw.plan_failed(*problem),
                    Mode::Search => {
                        self.search.problem = Some(*problem);
                        if self.search.focus == SearchPanel::Body {
                            self.search.focus = SearchPanel::Context;
                        }
                        self.search.definition_tab = false;
                    }
                    Mode::History(_) => self.status.problem = Some(*problem),
                }
                Vec::new()
            }
            Event::Applied { id, intent, report } => {
                self.mode = Mode::Search;
                self.search.trail.cancel();
                self.search.preview = None;
                self.search.source_anchor = None;
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
                self.search.trail.cancel();
                self.search.preview = None;
                self.search.source_anchor = None;
                self.search.body.clear();
                let effects = self.search();
                self.status.busy = false;
                self.status
                    .info(format!("↩ #{}  {}", entry.id, IntentLine(&entry.intent)));
                effects
            }
            Event::Failed {
                generation,
                problem,
            } => {
                if matches!(problem.retry, Some(Effect::Commit { .. }))
                    && !matches!(&self.mode, Mode::Rename(r) if r.applying)
                    && !matches!(&self.mode, Mode::Move(m) if m.applying)
                    && !matches!(&self.mode, Mode::Rewrite(r) if r.applying)
                {
                    return Vec::new();
                }
                if generation.is_some_and(|g| g != self.generation) {
                    return Vec::new();
                }
                if let Some(Effect::Preview { path }) = &problem.retry {
                    let site = match &self.mode {
                        Mode::Search => self.search.preview_target().map(|path| (path, 0)),
                        Mode::Rename(r) => r.site(),
                        Mode::Move(m) => m.site(),
                        Mode::Rewrite(r) => r.site(),
                        Mode::History(_) => None,
                    };
                    if site.as_ref().is_none_or(|(p, _)| p != path) {
                        return Vec::new();
                    }
                }
                self.status.busy = false;
                self.arriving = false;
                match &mut self.mode {
                    Mode::Rename(r) => r.plan_failed(*problem),
                    Mode::Move(m) => m.plan_failed(*problem),
                    Mode::Rewrite(r) => r.plan_failed(*problem),
                    Mode::Search => {
                        self.search.problem = Some(*problem);
                        if self.search.focus == SearchPanel::Body {
                            self.search.focus = SearchPanel::Context;
                        }
                        self.search.definition_tab = false;
                    }
                    Mode::History(_) => self.status.problem = Some(*problem),
                }
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
        if let Some(Overlay::Places(picker)) = &mut self.overlay {
            picker.moved(by);
            Vec::new()
        } else if let Some(Overlay::Navigation(picker)) = &mut self.overlay {
            picker.moved(by);
            Vec::new()
        } else if let Some(Overlay::Menu(menu)) = &mut self.overlay {
            menu.move_cursor(by);
            Vec::new()
        } else {
            self.mode_update(Action::Move(by))
        }
    }

    fn jump(&mut self, top: bool) -> Vec<Effect> {
        if matches!(self.overlay, Some(Overlay::Help { .. })) {
            return self.scrolled(if top { i32::MIN / 2 } else { i32::MAX / 2 });
        }
        if self.overlay.is_none()
            && matches!(self.mode, Mode::Search)
            && self.search.workspace.is_none()
            && self.search.scroll_focused()
        {
            self.search.jump(top)
        } else if self.overlay.is_none() && self.scroll_focused() {
            self.mode_update(if top { Action::Top } else { Action::Bottom })
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
            .is_some_and(|overlay| overlay.help_scrolled(by, self.viewport.unwrap_or((90, 20))))
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
        if let Some(Overlay::Places(picker)) = &self.overlay {
            let Some(place) = picker.chosen() else {
                return Vec::new();
            };
            self.overlay = None;
            let mut context = ModeContext {
                status: &mut self.status,
                generation: &mut self.generation,
            };
            let effects = match place {
                crate::overlays::places::Place::Trail(steps) => {
                    self.search.travel_steps(steps, &mut context)
                }
                crate::overlays::places::Place::Recent(recipe) => {
                    self.search.reopen(&recipe, &mut context)
                }
            };
            self.sync_viewport();
            return effects;
        }
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
        if let Some(Overlay::Help { previous, .. }) =
            self.overlay.take_if(|o| matches!(o, Overlay::Help { .. }))
        {
            self.overlay = previous.map(|o| *o);
            return Vec::new();
        }
        if self.overlay.take().is_some() {
            return Vec::new();
        }
        if !matches!(self.mode, Mode::Search) {
            self.mode = Mode::Search;
            self.arriving = false;
            self.status.clear();
            return self.preview_effect();
        }
        let effects = self.search.back(&mut ModeContext {
            status: &mut self.status,
            generation: &mut self.generation,
        });
        self.sync_viewport();
        effects
    }

    // ------------------------------------------------------------ modes

    /// A retained-hub selection follows the active mode's preview.
    fn navigation(&mut self, navigation: Navigation) -> Vec<Effect> {
        match navigation {
            Navigation::Selection => {
                self.search.selection_changed();
                self.sync_viewport();
                self.preview_effect()
            }
            Navigation::Effects(effects) => effects,
        }
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
            !self.status.busy && !self.search.stale,
        )));
        Vec::new()
    }

    fn follow(&mut self) -> Vec<Effect> {
        if !matches!(self.mode, Mode::Search) {
            return Vec::new();
        }
        let (picker, effects) = self.search.follow(&mut ModeContext {
            status: &mut self.status,
            generation: &mut self.generation,
        });
        if let Some(picker) = picker {
            self.overlay = Some(Overlay::Navigation(picker));
        }
        effects
    }

    fn choose_menu(&mut self) -> Vec<Effect> {
        if let Some(Overlay::Navigation(picker)) = &self.overlay {
            let Some(query) = picker.chosen() else {
                return Vec::new();
            };
            self.overlay = None;
            return self.search.follow_query(
                query,
                &mut ModeContext {
                    status: &mut self.status,
                    generation: &mut self.generation,
                },
            );
        }
        let Some(Overlay::Menu(menu)) = &self.overlay else {
            return Vec::new();
        };
        let Some(choice) = menu.chosen() else {
            return Vec::new();
        };
        if choice.0 == MenuTarget::Filters {
            match choice.1.as_deref() {
                Some("in") => return self.open_menu(MenuTarget::Location),
                Some("symbol") => return self.open_menu(MenuTarget::Symbol),
                Some("lang") => return self.open_menu(MenuTarget::Language),
                Some("category") => return self.open_menu(MenuTarget::Category),
                Some("files") => {
                    self.overlay = None;
                    return self.mode_update(Action::FilterFiles);
                }
                Some("node") => {
                    self.overlay = None;
                    return self.mode_update(Action::FocusNth(1));
                }
                _ => {}
            }
        }
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
        if let Some(path) = self.problem().and_then(crate::problem::Problem::site) {
            return vec![Effect::Edit { path, line: 0 }];
        }
        if matches!(self.overlay, Some(Overlay::Report { .. })) {
            return match self.report_site() {
                Some((path, line)) => vec![Effect::Edit { path, line }],
                None => Vec::new(),
            };
        }
        let site = match &self.mode {
            Mode::Search => self.search.site(),
            Mode::Rename(r) => r
                .site()
                .or_else(|| r.target.declared_in.clone().map(|path| (path, 0)))
                .or_else(|| self.search.site()),
            Mode::Move(mv) => mv.site().or_else(|| Some((mv.from.clone(), 0))),
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
    pub fn busy(&self) -> bool {
        match self {
            Self::Rename(r) => r.busy || r.applying,
            Self::Move(m) => m.busy || m.applying,
            Self::Rewrite(r) => r.busy || r.applying,
            _ => false,
        }
    }

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
    pub problem: Option<crate::problem::Problem>,
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
        self.problem = None;
    }
}

/// A file for a context or detail panel: its text, line boundaries, and
/// syntax colouring as byte spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePreview {
    file: std::sync::Arc<vvv_engine::File>,
    line_starts: std::sync::Arc<Vec<usize>>,
    content: vvv_engine::ContentId,
    /// Original highlight indices in start order, with prefix maximum ends.
    highlight_index: std::sync::Arc<Vec<(usize, usize)>>,
}

impl FilePreview {
    pub fn new(file: vvv_engine::File) -> Self {
        let line_starts = std::iter::once(0)
            .chain(file.text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let mut highlights: Vec<_> = (0..file.highlights.len()).collect();
        highlights.sort_by_key(|&index| file.highlights[index].span.start);
        let mut maximum = 0;
        let highlight_index = highlights
            .into_iter()
            .map(|index| {
                maximum = maximum.max(file.highlights[index].span.end);
                (index, maximum)
            })
            .collect();
        Self {
            content: vvv_engine::ContentId::of(&file.text),
            file: std::sync::Arc::new(file),
            line_starts: std::sync::Arc::new(line_starts),
            highlight_index: std::sync::Arc::new(highlight_index),
        }
    }

    /// A conservative payload charge, including syntax and anchor metadata.
    pub fn retained_bytes(&self) -> usize {
        let mut bytes = RetainedBytes::default();
        if serde_json::to_writer(&mut bytes, &*self.file).is_err() {
            return usize::MAX / 128;
        }
        bytes
            .estimate()
            .saturating_add(self.line_starts.len() * std::mem::size_of::<usize>())
            .saturating_add(128)
            .saturating_add(self.highlight_index.len() * std::mem::size_of::<(usize, usize)>())
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

    pub fn content_id(&self) -> &vvv_engine::ContentId {
        &self.content
    }

    /// Intersecting syntax spans in their original priority order, including
    /// overlapping or multiline highlights that start before this window.
    pub fn highlights_in(&self, start: usize, end: usize) -> Vec<&vvv_engine::Highlight> {
        if start >= end {
            return Vec::new();
        }
        let first = self
            .highlight_index
            .partition_point(|&(_, maximum)| maximum <= start);
        let last = self
            .highlight_index
            .partition_point(|&(index, _)| self.highlights[index].span.start < end);
        if first >= last {
            return Vec::new();
        }
        let mut indices: Vec<_> = self.highlight_index[first..last]
            .iter()
            .filter(|&&(index, _)| self.highlights[index].span.end > start)
            .map(|&(index, _)| index)
            .collect();
        indices.sort_unstable();
        indices
            .into_iter()
            .map(|index| &self.highlights[index])
            .collect()
    }
}

impl std::ops::Deref for FilePreview {
    type Target = vvv_engine::File;

    fn deref(&self) -> &Self::Target {
        &self.file
    }
}

/// Counts encoded payload without allocating a second copy. The multiplier
/// reserves space for collection elements, capacities and allocation overhead;
/// this is an estimated retention budget, not a process RSS measurement.
#[derive(Default)]
pub struct RetainedBytes(usize);
impl RetainedBytes {
    pub fn estimate(&self) -> usize {
        self.0.saturating_mul(4)
    }
}
impl std::io::Write for RetainedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
