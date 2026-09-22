//! One interactive session: the terminal, the event loop, the editor hand-off.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event as TermEvent, KeyEventKind};
use vvv_engine::{Engine, Retention};

use crate::action::{Action, Effect};
use crate::error::Error;
use crate::model::Model;
use crate::render::Painter;
use crate::screen::App;
use crate::worker::Worker;

/// How long an input must be idle before its search or plan is sent.
const DEBOUNCE: Duration = Duration::from_millis(120);
const TICK: Duration = Duration::from_millis(50);
/// How long a session trusts its last walk of the tree: long enough that
/// typing never re-walks per key, short enough that an edit made elsewhere
/// shows up by the next pause.
const TRUST: Duration = Duration::from_secs(1);

/// One interactive session over an engine.
pub struct Tui {
    engine: Engine,
    editor: Option<String>,
    color: bool,
}

impl Tui {
    /// A session asks the same tree question after question, so it keeps
    /// files and facts between them; only what changed on disk is re-read,
    /// and a burst of keystrokes walks the tree once, not per key.
    pub fn new(engine: Engine) -> Self {
        Self {
            engine: engine.with_retention(Retention::session().trusting(TRUST)),
            editor: None,
            color: true,
        }
    }

    /// The command `e` runs, given `+line path` — `hx`, `code --wait`,
    /// whatever `$VISUAL` or `$EDITOR` says. Without one, `e` says so.
    pub fn editor(mut self, command: Option<String>) -> Self {
        self.editor = command.filter(|c| !c.trim().is_empty());
        self
    }

    /// Colour, or bold and dim only.
    pub fn color(mut self, enabled: bool) -> Self {
        self.color = enabled;
        self
    }

    /// Take the terminal until the user quits.
    pub fn run(self) -> Result<(), Error> {
        let root = self.engine.root().to_path_buf();
        let languages = self
            .engine
            .language_ids()
            .iter()
            .map(ToString::to_string)
            .collect();
        let painter = if self.color {
            Painter::colored()
        } else {
            Painter::plain()
        };
        let editor = self.editor;
        let worker = Worker::spawn(self.engine);
        let mut model = Model::new(root.display().to_string(), languages);
        let mut terminal = ratatui::init();
        let outcome = Self::event_loop(
            &mut terminal,
            &mut model,
            &worker,
            painter,
            &root,
            editor.as_deref(),
        );
        ratatui::restore();
        outcome
    }

    fn event_loop(
        terminal: &mut ratatui::DefaultTerminal,
        model: &mut Model,
        worker: &Worker,
        painter: Painter,
        root: &Path,
        editor: Option<&str>,
    ) -> Result<(), Error> {
        // Searches and plans are debounced here, in the one place that
        // knows about time; the newest request of each kind wins.
        let mut pending: Option<(Effect, Instant)> = None;
        let mut effects = model.update(Action::Start);

        loop {
            for effect in effects.drain(..) {
                match effect {
                    Effect::Search { .. } | Effect::Plan { debounce: true, .. } => {
                        pending = Some((effect, Instant::now()));
                    }
                    Effect::Edit { path, line } => match editor {
                        Some(editor) => {
                            Self::edit(terminal, editor, root, &path, line)?;
                            // The editor may have written anything.
                            worker.send(Effect::Touched);
                        }
                        None => model.status.error("no editor: set $VISUAL or $EDITOR"),
                    },
                    other => worker.send(other),
                }
            }
            if pending
                .as_ref()
                .is_some_and(|(_, since)| since.elapsed() >= DEBOUNCE)
                && let Some((effect, _)) = pending.take()
            {
                worker.send(effect);
            }

            terminal.draw(|frame| frame.render_widget(App::new(model, painter), frame.area()))?;
            if model.quit {
                return Ok(());
            }

            if event::poll(TICK)? {
                match event::read()? {
                    TermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                        effects.extend(model.on_key(key));
                    }
                    TermEvent::Resize(..) => {}
                    _ => {}
                }
            }
            while let Some(event) = worker.try_recv() {
                effects.extend(model.on_event(event));
            }
        }
    }

    /// Hand the terminal to the editor at `path:line`, then take it back
    /// and redraw.
    fn edit(
        terminal: &mut ratatui::DefaultTerminal,
        editor: &str,
        root: &Path,
        path: &Path,
        line: u32,
    ) -> Result<(), Error> {
        let mut words = editor.split_whitespace();
        let Some(program) = words.next() else {
            return Ok(());
        };
        ratatui::restore();
        let status = Command::new(program)
            .args(words)
            .arg(format!("+{}", line + 1))
            .arg(root.join(path))
            .status();
        *terminal = ratatui::init();
        terminal.clear()?;
        status.map_err(|source| Error::Editor {
            command: editor.to_owned(),
            source,
        })?;
        Ok(())
    }
}
