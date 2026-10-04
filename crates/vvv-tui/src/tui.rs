//! One interactive session: the terminal, the event loop, the editor hand-off.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::crossterm::SynchronizedUpdate;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as TermEvent, KeyEventKind,
};
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
        let root = std::fs::canonicalize(&root).unwrap_or(root);
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
        let preferences = PreferenceFile::new(&root);
        if let Some(file) = &preferences {
            file.load(&mut model);
        }
        let mut terminal = ratatui::init();
        let capture = TerminalInputCapture::new()?;
        let outcome = Self::event_loop(
            &mut terminal,
            &mut model,
            &worker,
            painter,
            &root,
            editor.as_deref(),
        );
        drop(capture);
        ratatui::restore();
        outcome?;
        if let Some(file) = preferences {
            file.save(&model).map_err(Error::Preferences)?;
        }
        Ok(())
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
        let size = terminal.size()?;
        model.on_event(crate::action::Event::Viewport {
            width: size.width,
            height: size.height,
        });
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
                            model.on_event(crate::action::Event::SourcesChanged);
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

            let now = vvv_engine::protocol::vocabulary::Ago::now();
            // Present the cell diff as one frame on supporting terminals. The
            // end marker is sent even when drawing returns an I/O error.
            std::io::stdout().sync_update(|_| {
                terminal
                    .draw(|frame| frame.render_widget(App::new(model, painter, now), frame.area()))
                    .map(|_| ())
            })??;
            if model.quit {
                return Ok(());
            }

            let frame = model.search_frame();
            if event::poll(TICK)? {
                match event::read()? {
                    TermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                        effects.extend(model.on_key(key));
                    }
                    TermEvent::Paste(text) => {
                        effects.extend(model.on_event(crate::action::Event::Paste(text)));
                    }
                    TermEvent::Mouse(mouse) => {
                        if let Some(pointer) = frame.pointer(mouse) {
                            effects.extend(model.on_event(crate::action::Event::Pointer(pointer)));
                        }
                    }
                    TermEvent::Resize(width, height) => {
                        effects.extend(
                            model.on_event(crate::action::Event::Viewport { width, height }),
                        );
                    }
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
        ratatui::crossterm::execute!(
            std::io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste
        )?;
        ratatui::restore();
        let status = Command::new(program)
            .args(words)
            .arg(format!("+{}", line + 1))
            .arg(root.join(path))
            .status();
        *terminal = ratatui::init();
        ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;
        terminal.clear()?;
        status.map_err(|source| Error::Editor {
            command: editor.to_owned(),
            source,
        })?;
        Ok(())
    }
}

/// Mouse reporting follows the terminal session, including error unwinding.
struct TerminalInputCapture {
    enabled: bool,
}
impl TerminalInputCapture {
    fn new() -> Result<Self, Error> {
        let capture = Self { enabled: true };
        if let Err(error) = ratatui::crossterm::execute!(
            std::io::stdout(),
            EnableMouseCapture,
            EnableBracketedPaste
        ) {
            drop(capture);
            ratatui::restore();
            return Err(error.into());
        }
        Ok(capture)
    }
}
impl Drop for TerminalInputCapture {
    fn drop(&mut self) {
        if self.enabled {
            let _ = ratatui::crossterm::execute!(
                std::io::stdout(),
                DisableMouseCapture,
                DisableBracketedPaste
            );
        }
    }
}

/// UI data stays outside the project tree and is keyed by canonical workspace.
struct PreferenceFile {
    path: std::path::PathBuf,
}
impl PreferenceFile {
    fn new(root: &Path) -> Option<Self> {
        use std::hash::{Hash, Hasher};
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                if cfg!(windows) {
                    std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from)
                } else {
                    std::env::var_os("HOME").map(|home| {
                        let home = std::path::PathBuf::from(home);
                        if cfg!(target_os = "macos") {
                            home.join("Library/Application Support")
                        } else {
                            home.join(".local/state")
                        }
                    })
                }
            })?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        root.hash(&mut hash);
        Some(Self {
            path: state
                .join("vvv/tui")
                .join(format!("{:016x}.json", hash.finish())),
        })
    }
    fn load(&self, model: &mut Model) {
        let Ok(metadata) = std::fs::metadata(&self.path) else {
            return;
        };
        if metadata.len() > 256 * 1024 {
            return;
        }
        if let Ok(bytes) = std::fs::read(&self.path)
            && let Some(preferences) = crate::preferences::Preferences::decode(&bytes, &model.root)
        {
            preferences.restore(model);
        }
    }
    fn save(&self, model: &Model) -> std::io::Result<()> {
        let directory = self.path.parent().expect("preference file has a parent");
        std::fs::create_dir_all(directory)?;
        std::fs::write(
            &self.path,
            crate::preferences::Preferences::capture(model).encode(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preference_storage_round_trip_is_workspace_bound_and_rejects_corrupt_or_oversized_files() {
        let directory = std::env::temp_dir().join(format!(
            "vvv-tui-preferences-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file = PreferenceFile {
            path: directory.join("workspace.json"),
        };
        let mut model = Model::new("/workspace".into(), vec![]);
        model.split = 67;
        file.save(&model).unwrap();
        let mut restored = Model::new("/workspace".into(), vec![]);
        file.load(&mut restored);
        assert_eq!(restored.split, 67);
        let mut other = Model::new("/other".into(), vec![]);
        file.load(&mut other);
        assert_eq!(other.split, 50);
        std::fs::write(&file.path, b"broken").unwrap();
        let mut fresh = Model::new("/workspace".into(), vec![]);
        file.load(&mut fresh);
        assert_eq!(fresh.split, 50);
        std::fs::write(&file.path, vec![b' '; 256 * 1024 + 1]).unwrap();
        file.load(&mut fresh);
        assert_eq!(fresh.split, 50);
        std::fs::remove_file(&file.path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }
}
