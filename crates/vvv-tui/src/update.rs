//! How the model changes: keys become [`Action`]s through the keymap, by
//! the mode and the focused panel; actions and engine [`Event`]s change the
//! [`Model`] and may ask for [`Effect`]s. No I/O here; everything is
//! testable with plain assertions.

use ratatui::crossterm::event::KeyEvent;
use vvv_engine::{Confidence, Intent, RenameIntent, Selection, SymbolKind};

use super::action::{Action, Effect, Event, Planned};
use super::keymap::{Dispatch, Key};
use super::model::{
    Confirm, Confirmed, FilePreview, HistoryMode, HistoryPanel, Menu, MenuTarget, Mode, Model,
    MoveMode, MovePanel, MovePlan, Overlay, Panels, RenameMode, RenamePanel, RewriteMode,
    RewritePanel, SearchPanel,
};
use super::query::Filter;
use vvv_engine::protocol::vocabulary::IntentLine;

impl Model {
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
            Action::FocusNext => self.focus_by(1),
            Action::FocusPrev => self.focus_by(-1),
            Action::FocusNth(n) => self.focus_nth(n),
            Action::Move(n) => {
                if matches!(self.overlay, Some(Overlay::Report { .. })) {
                    self.report_moved(n);
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
            Action::Toggle => self.toggled(false),
            Action::ToggleAll => self.toggled(true),
            Action::View => {
                self.view = self.view.toggled();
                Vec::new()
            }
            Action::Input(c) => self.input(Some(c)),
            Action::Backspace => self.input(None),
            Action::Clear => {
                if let Mode::Search = self.mode {
                    self.search.query.clear();
                    return self.search();
                }
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
            Action::Diff => {
                if let Mode::Move(mv) = &mut self.mode {
                    mv.diff = !mv.diff;
                    mv.detail_scroll = 0;
                }
                Vec::new()
            }
            Action::Edit => self.edit(),
        }
    }

    pub fn on_event(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Searched {
                generation,
                matches,
                skipped,
            } => {
                if generation != self.generation {
                    return Vec::new();
                }
                self.status.busy = false;
                self.search.results.replace(matches);
                if skipped.is_empty() {
                    self.status.clear();
                } else {
                    let names: Vec<String> =
                        skipped.iter().map(|s| s.language.to_string()).collect();
                    self.status.info(format!(
                        "{} skipped: the pattern does not parse there",
                        names.join(", ")
                    ));
                }
                self.preview_effect()
            }
            Event::Previewed {
                path,
                text,
                highlights,
            } => {
                let preview = FilePreview::new(path, text, highlights);
                match &mut self.mode {
                    Mode::Search => {
                        self.search.preview = Some(preview);
                        self.search.preview_scroll = None;
                    }
                    Mode::Rename(r) => {
                        r.preview = Some(preview);
                        r.detail_scroll = 0;
                    }
                    Mode::Move(mv) => {
                        mv.preview = Some(preview);
                        mv.detail_scroll = 0;
                    }
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
                    Mode::Rename(r) => {
                        r.busy = false;
                        r.error = Some(message);
                    }
                    Mode::Move(mv) => {
                        mv.busy = false;
                        mv.plan = None;
                        mv.error = Some(message);
                    }
                    Mode::Rewrite(rw) => {
                        rw.busy = false;
                        rw.changes.clear();
                        rw.error = Some(message);
                    }
                    Mode::Search | Mode::History(_) => self.status.error(message),
                }
                Vec::new()
            }
            Event::Applied { id, intent, report } => {
                self.mode = Mode::Search;
                self.search.preview = None;
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
                    Mode::Rename(r) => r.busy = false,
                    Mode::Move(mv) => mv.busy = false,
                    Mode::Rewrite(rw) => rw.busy = false,
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
            ) => {
                r.declarations = declarations;
                r.occurrences = occurrences;
                r.changes = files;
                r.busy = false;
                r.error = None;
                // The first plan seeds the ticks from the engine's default:
                // what its plan with no selection edits. Later plans (typing
                // the new name) only refresh the diff, so the ticks stand.
                if !r.judged {
                    r.ticks = r
                        .occurrences
                        .iter()
                        .filter(|o| {
                            r.changes.iter().any(|f| {
                                f.path == o.m.path && f.edits.iter().any(|e| e.span == o.m.span)
                            })
                        })
                        .map(|o| o.m.id.clone())
                        .collect();
                    r.judged = true;
                    for cursor in &mut r.cursors {
                        cursor.index = 0;
                    }
                    // Start where judgment is needed; `✓` when there is nothing to judge.
                    r.last_list = if r.rows(Confidence::Unresolved).is_empty() {
                        RenamePanel::Sure
                    } else {
                        RenamePanel::Unsure
                    };
                }
                self.preview_effect()
            }
            (
                Mode::Move(mv),
                Planned::Move {
                    intent,
                    addresses,
                    respellings,
                    notices,
                    files,
                },
            ) => {
                mv.plan = Some(MovePlan::new(
                    intent,
                    files,
                    respellings,
                    notices,
                    addresses,
                ));
                mv.error = None;
                mv.busy = false;
                for panel in [
                    MovePanel::Respellings,
                    MovePanel::Structural,
                    MovePanel::Notices,
                ] {
                    let len = mv.len(panel);
                    if let Some(cursor) = mv.cursor_mut(panel) {
                        cursor.clamp(len);
                    }
                }
                mv.last_list = if mv.len(MovePanel::Respellings) > 0 {
                    MovePanel::Respellings
                } else {
                    MovePanel::Structural
                };
                self.preview_effect()
            }
            (Mode::Rewrite(rw), Planned::Rewrite { files }) => {
                rw.changes = files;
                rw.error = None;
                rw.busy = false;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    // ------------------------------------------------------------ focus

    fn focus_by(&mut self, by: i32) -> Vec<Effect> {
        match &mut self.mode {
            Mode::Search => self.search.focus = self.search.focus.step(by),
            Mode::Rename(r) => r.focus = r.focus.step(by),
            Mode::Move(mv) => mv.focus = mv.focus.step(by),
            Mode::Rewrite(rw) => rw.focus = rw.focus.step(by),
            Mode::History(h) => h.focus = h.focus.step(by),
        }
        self.preview_effect()
    }

    fn focus_nth(&mut self, n: u8) -> Vec<Effect> {
        match &mut self.mode {
            Mode::Search => {
                if let Some(p) = SearchPanel::nth(n) {
                    self.search.focus = p;
                }
            }
            Mode::Rename(r) => {
                if let Some(p) = RenamePanel::nth(n) {
                    r.focus = p;
                }
            }
            Mode::Move(mv) => {
                if let Some(p) = MovePanel::nth(n) {
                    mv.focus = p;
                }
            }
            Mode::Rewrite(rw) => {
                if let Some(p) = RewritePanel::nth(n) {
                    rw.focus = p;
                }
            }
            Mode::History(h) => {
                if let Some(p) = HistoryPanel::nth(n) {
                    h.focus = p;
                }
            }
        }
        self.preview_effect()
    }

    // ------------------------------------------------------------ cursors

    fn moved(&mut self, by: i32) -> Vec<Effect> {
        if let Some(Overlay::Menu(menu)) = &mut self.overlay {
            menu.move_cursor(by);
            return Vec::new();
        }
        match &mut self.mode {
            Mode::Search => {
                let len = self.search.results.matches.len();
                self.search.results.cursor.move_by(by, len);
                self.search.preview_scroll = None;
            }
            Mode::Rename(r) => {
                let panel = r.list();
                if let Some(confidence) = panel.confidence() {
                    let len = r.rows(confidence).len();
                    if let Some(cursor) = r.cursor_mut(panel) {
                        cursor.move_by(by, len);
                    }
                    r.detail_scroll = 0;
                }
            }
            Mode::Move(mv) => {
                let panel = mv.list();
                let len = mv.len(panel);
                if let Some(cursor) = mv.cursor_mut(panel) {
                    cursor.move_by(by, len);
                }
                mv.detail_scroll = 0;
            }
            Mode::Rewrite(rw) => {
                let len = rw.matches.len();
                rw.cursor.move_by(by, len);
                rw.detail_scroll = 0;
            }
            Mode::History(h) => {
                let len = h.entries.len();
                h.cursor.move_by(by, len);
                h.files_scroll = 0;
            }
        }
        self.preview_effect()
    }

    fn jump(&mut self, top: bool) -> Vec<Effect> {
        let far = if top { i32::MIN / 2 } else { i32::MAX / 2 };
        match (&self.mode, self.scroll_focused()) {
            (Mode::Search, true) => {
                self.search.preview_scroll = Some(if top {
                    0
                } else {
                    self.search.preview.as_ref().map_or(0, |p| p.line_count())
                });
                Vec::new()
            }
            _ => self.moved(far),
        }
    }

    /// Whether the focused panel is a text panel that scrolls rather than
    /// a list with a cursor.
    fn scroll_focused(&self) -> bool {
        match &self.mode {
            Mode::Search => self.search.focus == SearchPanel::Context,
            Mode::Rename(r) => r.focus == RenamePanel::Detail,
            Mode::Move(mv) => mv.focus == MovePanel::Detail,
            Mode::Rewrite(rw) => rw.focus == RewritePanel::Detail,
            Mode::History(h) => h.focus == HistoryPanel::Files,
        }
    }

    fn scrolled(&mut self, by: i32) -> Vec<Effect> {
        let bump = |scroll: &mut usize| *scroll = (*scroll as i32 + by).max(0) as usize;
        if let Some(Overlay::Help { scroll, .. }) = &mut self.overlay {
            bump(scroll);
            return Vec::new();
        }
        if !self.scroll_focused() {
            return self.moved(by);
        }
        match &mut self.mode {
            Mode::Search => {
                let max = self
                    .search
                    .preview
                    .as_ref()
                    .map_or(0, |p| p.line_count().saturating_sub(1));
                let current = self
                    .search
                    .preview_scroll
                    .unwrap_or_else(|| self.preview_anchor());
                self.search.preview_scroll =
                    Some((current as i32 + by).clamp(0, max as i32) as usize);
            }
            Mode::Rename(r) => bump(&mut r.detail_scroll),
            Mode::Move(mv) => bump(&mut mv.detail_scroll),
            Mode::Rewrite(rw) => bump(&mut rw.detail_scroll),
            Mode::History(h) => bump(&mut h.files_scroll),
        }
        Vec::new()
    }

    /// The line the search context centres on when following the cursor.
    pub fn preview_anchor(&self) -> usize {
        self.search
            .results
            .current()
            .map_or(0, |m| (m.start.line as usize).saturating_sub(5))
    }

    /// The file the focused row is in, if the mode's detail does not show
    /// it yet.
    fn preview_effect(&self) -> Vec<Effect> {
        let (path, shown) = match &self.mode {
            Mode::Search => (
                self.search.results.current().map(|m| m.path.clone()),
                self.search.preview.as_ref().map(|p| p.path.clone()),
            ),
            Mode::Rename(r) => (
                r.current().map(|o| o.m.path.clone()),
                r.preview.as_ref().map(|p| p.path.clone()),
            ),
            Mode::Move(mv) => (
                mv.current().map(|row| row.path().into()),
                mv.preview.as_ref().map(|p| p.path.clone()),
            ),
            // Rewrite's preview is the plan's diff, not a source file.
            Mode::Rewrite(_) => (None, None),
            Mode::History(_) => (None, None),
        };
        match path {
            Some(path) if shown.as_ref() != Some(&path) => vec![Effect::Preview { path }],
            _ => Vec::new(),
        }
    }

    // ------------------------------------------------------------ toggles

    fn toggled(&mut self, all: bool) -> Vec<Effect> {
        match &mut self.mode {
            Mode::Rename(r) => {
                if all {
                    r.toggle_panel();
                } else {
                    r.toggle();
                    return self.moved(1);
                }
            }
            Mode::Rewrite(rw) => {
                if all {
                    rw.toggle_all();
                } else {
                    rw.toggle();
                    return self.moved(1);
                }
            }
            Mode::Search | Mode::Move(_) | Mode::History(_) => {}
        }
        Vec::new()
    }

    // ------------------------------------------------------------ inputs

    /// A character typed (or, with `None`, erased) in the mode's input.
    fn input(&mut self, c: Option<char>) -> Vec<Effect> {
        fn edit(text: &mut String, c: Option<char>) {
            match c {
                Some(c) => text.push(c),
                None => {
                    text.pop();
                }
            }
        }
        match &mut self.mode {
            Mode::Search => {
                match c {
                    Some(c) => self.search.query.push(c),
                    None => self.search.query.pop(),
                }
                self.search()
            }
            Mode::Rename(r) => {
                edit(&mut r.name, c);
                self.plan_rename(true)
            }
            Mode::Move(mv) => {
                edit(&mut mv.to, c);
                self.plan_move(true)
            }
            Mode::Rewrite(rw) => {
                edit(&mut rw.template, c);
                self.plan_rewrite()
            }
            Mode::History(_) => Vec::new(),
        }
    }

    fn search(&mut self) -> Vec<Effect> {
        let generation = self.next_generation();
        match self.search.query.parse() {
            Ok(query) => {
                self.status.busy = true;
                self.status.clear();
                self.search.results.query = Some(query.clone());
                vec![Effect::Search { generation, query }]
            }
            Err(e) => {
                self.search.results.replace(Vec::new());
                self.search.results.query = None;
                if self.search.query.is_empty() {
                    self.status.clear();
                } else {
                    self.status.error(e.to_string());
                }
                Vec::new()
            }
        }
    }

    fn plan_rename(&mut self, debounce: bool) -> Vec<Effect> {
        let generation = self.next_generation();
        let Mode::Rename(r) = &mut self.mode else {
            return Vec::new();
        };
        if r.name.trim().is_empty() {
            r.changes.clear();
            r.error = None;
            r.busy = false;
            return Vec::new();
        }
        let mut intent = RenameIntent::new(&r.target.name, r.name.trim());
        intent.symbol = r.target.symbol;
        intent.language = r.language.clone();
        intent.declared_in = r.target.declared_in.clone();
        r.busy = true;
        vec![Effect::Plan {
            generation,
            intent: Intent::Rename(intent),
            debounce,
        }]
    }

    fn plan_move(&mut self, debounce: bool) -> Vec<Effect> {
        let generation = self.next_generation();
        let Mode::Move(mv) = &mut self.mode else {
            return Vec::new();
        };
        match mv.intent() {
            Some(intent) => {
                mv.busy = true;
                vec![Effect::Plan {
                    generation,
                    intent,
                    debounce,
                }]
            }
            None => {
                mv.plan = None;
                mv.error = None;
                mv.busy = false;
                Vec::new()
            }
        }
    }

    fn plan_rewrite(&mut self) -> Vec<Effect> {
        let generation = self.next_generation();
        let Mode::Rewrite(rw) = &mut self.mode else {
            return Vec::new();
        };
        match rw.intent() {
            Some(intent) => {
                rw.busy = true;
                vec![Effect::Plan {
                    generation,
                    intent: Intent::Rewrite(intent),
                    debounce: true,
                }]
            }
            None => {
                rw.changes.clear();
                rw.error = None;
                rw.busy = false;
                Vec::new()
            }
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
        match &mut self.mode {
            Mode::Search => {
                self.search.focus = SearchPanel::Results;
                Vec::new()
            }
            Mode::Rename(r) => {
                if r.busy || r.occurrences.is_empty() {
                    return Vec::new();
                }
                let to = r.name.trim().to_owned();
                if to.is_empty() || to == r.target.name {
                    return self.fail("type the new name first");
                }
                if r.ticks.is_empty() {
                    return self.fail("nothing ticked");
                }
                let mut intent = RenameIntent::new(&r.target.name, to)
                    .selecting(Selection::Ids(r.ticks.clone()));
                intent.symbol = r.target.symbol;
                intent.language = r.language.clone();
                intent.declared_in = r.target.declared_in.clone();
                r.busy = true;
                self.status.busy = true;
                vec![Effect::Commit {
                    intent: Intent::Rename(intent),
                }]
            }
            Mode::Move(mv) => match &mv.plan {
                Some(plan) if !mv.busy => {
                    mv.busy = true;
                    self.status.busy = true;
                    vec![Effect::Commit {
                        intent: plan.intent.clone(),
                    }]
                }
                _ => Vec::new(),
            },
            Mode::Rewrite(rw) => {
                if rw.busy || rw.changes.is_empty() {
                    return self.fail("type a template first");
                }
                if rw.ticks.is_empty() {
                    return self.fail("nothing ticked");
                }
                let Some(intent) = rw.intent() else {
                    return Vec::new();
                };
                rw.busy = true;
                self.status.busy = true;
                vec![Effect::Commit {
                    intent: Intent::Rewrite(intent.selecting(Selection::Ids(rw.ticks.clone()))),
                }]
            }
            Mode::History(_) => self.undo_requested(),
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
        Vec::new()
    }

    // ------------------------------------------------------------ modes

    fn enter_rename(&mut self) -> Vec<Effect> {
        let Some(target) = self.search.results.rename_target() else {
            return self.fail("put the cursor on an identifier or a declaration to rename");
        };
        let language = self
            .search
            .results
            .query
            .as_ref()
            .and_then(|q| q.language().cloned());
        // Judge with the name unchanged: verdicts do not depend on the new one.
        let mut intent = RenameIntent::new(&target.name, &target.name);
        intent.symbol = target.symbol;
        intent.language = language.clone();
        intent.declared_in = target.declared_in.clone();
        self.mode = Mode::Rename(Box::new(RenameMode::new(target, language)));
        self.arriving = true;
        let generation = self.next_generation();
        vec![Effect::Plan {
            generation,
            intent: Intent::Rename(intent),
            debounce: false,
        }]
    }

    fn enter_move(&mut self, symbol: bool) -> Vec<Effect> {
        let Some(m) = self.search.results.current() else {
            return self.fail("put the cursor on a match in the file to move");
        };
        let name = if symbol {
            match m.symbol.as_ref().filter(|s| s.kind != SymbolKind::Impl) {
                Some(s) => Some(s.name.clone()),
                None => return self.fail("put the cursor on a declaration to move it"),
            }
        } else {
            None
        };
        self.mode = Mode::Move(Box::new(MoveMode::new(m.path.clone(), name)));
        let effects = self.plan_move(false);
        self.arriving = !effects.is_empty();
        effects
    }

    fn enter_rewrite(&mut self) -> Vec<Effect> {
        let Some(query) = self.search.results.query.clone() else {
            return self.fail("search for the pattern to rewrite first");
        };
        if self.search.results.matches.is_empty() {
            return self.fail("nothing matched; a rewrite acts on the matches");
        }
        let matches = self.search.results.matches.clone();
        self.mode = Mode::Rewrite(Box::new(RewriteMode::new(query, matches)));
        self.preview_effect()
    }

    fn open_menu(&mut self, target: MenuTarget) -> Vec<Effect> {
        let (filter, values) = match target {
            MenuTarget::Symbol => (
                Filter::Symbol,
                SymbolKind::ALL
                    .iter()
                    .map(|k| k.as_str().to_owned())
                    .collect(),
            ),
            MenuTarget::Language => (Filter::Lang, self.languages.clone()),
        };
        let current = self.search.query.filter(filter).map(str::to_owned);
        self.overlay = Some(Overlay::Menu(Menu::new(target, values, current.as_deref())));
        Vec::new()
    }

    fn choose_menu(&mut self) -> Vec<Effect> {
        let Some(Overlay::Menu(menu)) = &self.overlay else {
            return Vec::new();
        };
        let filter = match menu.target {
            MenuTarget::Symbol => Filter::Symbol,
            MenuTarget::Language => Filter::Lang,
        };
        let value = menu.current().value.clone();
        self.search.query.set_filter(filter, value.as_deref());
        self.overlay = None;
        self.search()
    }

    fn undo_requested(&mut self) -> Vec<Effect> {
        match &self.mode {
            Mode::History(h) => match h.current() {
                Some(entry) if h.is_newest() => {
                    self.overlay = Some(Overlay::Confirm(Confirm {
                        question: format!("↩ #{}  {}?", entry.id, IntentLine(&entry.intent)),
                        then: Confirmed::Undo,
                    }));
                    Vec::new()
                }
                Some(_) => self.fail("only the newest entry can be undone"),
                None => self.fail("nothing to undo"),
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
            Mode::Search => self
                .search
                .results
                .current()
                .map(|m| (m.path.clone(), m.start.line)),
            Mode::Rename(r) => r.current().map(|o| (o.m.path.clone(), o.m.start.line)),
            Mode::Move(mv) => mv.current().map(|row| (row.path().into(), row.line())),
            Mode::Rewrite(rw) => rw.current().map(|m| (m.path.clone(), m.start.line)),
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
