//! The engine on its own thread. Effects go in, events come out; the UI
//! thread never blocks on a search or a plan. Consecutive searches (or
//! plans) are coalesced so a burst of keystrokes runs the last one only.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use std::error::Error as _;

use vvv_engine::report::{Document, Options};
use vvv_engine::{Answer, Apply, Engine, EngineError, FileQuery, HistoryQuery, Intent, UndoLast};

use super::action::{Effect, Event, Planned};

pub struct Worker {
    effects: Sender<Effect>,
    events: Receiver<Event>,
}

impl Worker {
    pub fn spawn(engine: Engine) -> Self {
        let (effects, inbox) = mpsc::channel::<Effect>();
        let (outbox, events) = mpsc::channel::<Event>();
        thread::spawn(move || {
            let runner = Runner { engine, outbox };
            while let Ok(mut effect) = inbox.recv() {
                // Drop searches and plans superseded while we were busy.
                while let Ok(next) = inbox.try_recv() {
                    match (&effect, &next) {
                        (Effect::Search { .. }, Effect::Search { .. })
                        | (Effect::Plan { .. }, Effect::Plan { .. }) => effect = next,
                        _ => {
                            runner.run(effect);
                            effect = next;
                        }
                    }
                }
                runner.run(effect);
            }
        });
        Self { effects, events }
    }

    pub fn send(&self, effect: Effect) {
        // A closed channel means the thread died; the loop will notice on
        // the next event and there is nothing better to do here.
        let _ = self.effects.send(effect);
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}

struct Runner {
    engine: Engine,
    outbox: Sender<Event>,
}

/// Why an effect produced no answer: the engine refused, or the picker was
/// asked for something only the CLI does.
#[derive(Debug)]
enum Failure {
    Engine(EngineError),
    Unsupported(&'static str),
}

impl From<EngineError> for Failure {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}

impl std::fmt::Display for Failure {
    /// The whole chain, `cause: cause: cause`, the way the CLI prints it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(what) => f.write_str(what),
            Self::Engine(error) => {
                write!(f, "{error}")?;
                let mut source = error.source();
                while let Some(cause) = source {
                    write!(f, ": {cause}")?;
                    source = cause.source();
                }
                Ok(())
            }
        }
    }
}

impl Runner {
    fn run(&self, effect: Effect) {
        if matches!(effect, Effect::Touched) {
            self.engine.touched();
            return;
        }
        let event = match effect {
            // A plan that cannot be made is an answer, not a failure.
            Effect::Plan {
                generation, intent, ..
            } => match self.plan(intent) {
                Ok(planned) => Event::Planned {
                    generation,
                    planned,
                },
                Err(error) => Event::PlanFailed {
                    generation,
                    message: error.to_string(),
                },
            },
            other => match self.execute(other) {
                Ok(event) => event,
                Err(error) => Event::Failed(error.to_string()),
            },
        };
        let _ = self.outbox.send(event);
    }

    fn plan(&self, intent: Intent) -> Result<Planned, Failure> {
        let engine = &self.engine;
        let answer = engine.run(intent.clone())?.into_inner();
        Ok(match answer {
            Answer::Rename(r) => Planned::Rename {
                files: r.files,
                declarations: r.declarations,
                occurrences: r.occurrences,
            },
            Answer::Move(mv) => Planned::Move {
                files: mv.files,
                intent,
                respellings: mv.respellings,
                notices: mv.notices,
            },
            Answer::MoveSymbol(mv) => Planned::Move {
                files: mv.files,
                intent,
                respellings: mv.respellings,
                notices: mv.notices,
            },
            Answer::Rewrite(rw) => Planned::Rewrite { files: rw.files },
            _ => {
                return Err(Failure::Unsupported(
                    "the picker plans one command at a time; use `vvv batch`",
                ));
            }
        })
    }

    fn execute(&self, effect: Effect) -> Result<Event, Failure> {
        Ok(match effect {
            Effect::Search { generation, query } => {
                let search = self.engine.run(query.clone())?;
                Event::Searched {
                    generation,
                    matches: search.matches,
                    skipped: search.skipped,
                }
            }
            Effect::Preview { path } => {
                let file = self.engine.run(FileQuery { path: path.clone() })?;
                Event::Previewed {
                    text: file.text,
                    highlights: file.highlights,
                    path,
                }
            }
            Effect::Commit { intent } => {
                let engine = &self.engine;
                let answer = engine.run(Apply(engine.run(intent.clone())?))?;
                let report = Document::of(&answer, Options::default());
                Event::Applied {
                    id: answer.history_id().unwrap_or_default(),
                    intent,
                    report,
                }
            }
            Effect::History => Event::History(self.engine.run(HistoryQuery)?.entries),
            Effect::Undo => Event::Undone(self.engine.run(UndoLast)?.undone),
            // Planned above; the loop runs `Edit` itself; `Touched` is
            // handled before anything is answered.
            Effect::Plan { .. } | Effect::Edit { .. } | Effect::Touched => {
                return Err(Failure::Unsupported("not a worker effect"));
            }
        })
    }
}
