//! How the model changes: keys become [`Action`]s through the keymap, by
//! the mode and the focused panel; actions and engine [`Event`]s change the
//! [`Model`] and may ask for [`Effect`]s. No I/O here; everything is
//! testable with plain assertions.

use ratatui::crossterm::event::KeyEvent;
use vvv_engine::{Intent, SymbolKind};

use super::action::{Action, Effect, Event, Planned};
use super::keymap::{Dispatch, Key};
use super::model::{Confirmed, FilePreview, Menu, MenuTarget, Mode, Model, Overlay};
use crate::modes::context::ModeContext;
use crate::modes::search::{Navigation, Relation, query::Filter};
use crate::modes::{
    history::HistoryMode, moves::MoveMode, rename::RenameMode, rewrite::RewriteMode,
};
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
            } => {
                let preview = FilePreview::new(path, text, highlights);
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
        if let Some(Overlay::Help { scroll, .. }) = &mut self.overlay {
            *scroll = (*scroll as i32 + by).max(0) as usize;
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
    fn navigation(&self, navigation: Navigation) -> Vec<Effect> {
        match navigation {
            Navigation::Selection => self.preview_effect(),
            Navigation::Effects(effects) => effects,
        }
    }
    fn choose_relation(&mut self, relation: Relation) -> Vec<Effect> {
        let navigation = self.search.choose_relation(
            relation,
            &mut ModeContext {
                status: &mut self.status,
                generation: &mut self.generation,
            },
        );
        self.navigation(navigation)
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
        let menu = match target {
            MenuTarget::Symbol => {
                let values = SymbolKind::ALL
                    .iter()
                    .map(|k| k.as_str().to_owned())
                    .collect();
                let current = self.search.query.filter(Filter::Symbol).map(str::to_owned);
                Menu::new(target, values, current.as_deref())
            }
            MenuTarget::Language => {
                let current = self.search.query.filter(Filter::Lang).map(str::to_owned);
                Menu::new(target, self.languages.clone(), current.as_deref())
            }
            MenuTarget::Relation => Menu::relations(self.search.results.relation),
        };
        self.overlay = Some(Overlay::Menu(menu));
        Vec::new()
    }

    fn choose_menu(&mut self) -> Vec<Effect> {
        let Some(Overlay::Menu(menu)) = &self.overlay else {
            return Vec::new();
        };
        let target = menu.target;
        let value = menu.current().value.clone();
        self.overlay = None;
        match target {
            MenuTarget::Symbol => {
                self.search
                    .query
                    .set_filter(Filter::Symbol, value.as_deref());
                self.search()
            }
            MenuTarget::Language => {
                self.search.query.set_filter(Filter::Lang, value.as_deref());
                self.search()
            }
            MenuTarget::Relation => {
                let relation = value
                    .as_deref()
                    .and_then(Relation::from_key)
                    .unwrap_or_default();
                self.choose_relation(relation)
            }
        }
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
            Mode::Search => self.search.results.current_site(),
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
