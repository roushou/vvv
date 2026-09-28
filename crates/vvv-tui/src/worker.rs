//! The engine on its own thread. Effects go in, events come out; the UI
//! thread never blocks on a search or a plan. Consecutive searches (or
//! plans) are coalesced so a burst of keystrokes runs the last one only.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use std::error::Error as _;

use vvv_engine::report::Document;
use vvv_engine::{
    Answer, Engine, EngineError, FileQuery, Intent, Ledger, MutationAnswer, SearchQuery,
};

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
        let answer = engine
            .run(intent.clone().into_request(false))?
            .into_preview()?
            .into_inner();
        Ok(match answer {
            MutationAnswer::Rename(r) => Planned::Rename {
                files: r.files,
                declarations: r.declarations,
                occurrences: r.occurrences,
            },
            MutationAnswer::Move(mv) => Planned::Move {
                files: mv.files,
                intent,
                respellings: mv.respellings,
                notices: mv.notices,
            },
            MutationAnswer::MoveSymbol(mv) => Planned::Move {
                files: mv.files,
                intent,
                respellings: mv.respellings,
                notices: mv.notices,
            },
            MutationAnswer::Rewrite(rw) => Planned::Rewrite { files: rw.files },
            MutationAnswer::Batch(_) => {
                return Err(Failure::Unsupported(
                    "the picker plans one command at a time; use `vvv batch`",
                ));
            }
        })
    }

    fn execute(&self, effect: Effect) -> Result<Event, Failure> {
        Ok(match effect {
            Effect::Search { generation, query } => {
                let search = SearchQuery::from(query.clone()).execute(&self.engine)?;
                Event::Searched {
                    generation,
                    matches: search.matches,
                    skipped: search.skipped,
                }
            }
            Effect::Query {
                generation,
                request,
            } => {
                if !request.is_read_only() {
                    return Err(Failure::Unsupported(
                        "the hub asks read-only questions; a mutation is planned",
                    ));
                }
                let answer = self.engine.run(request)?.into_answer();
                Event::Answered {
                    generation,
                    answer: Box::new(answer),
                }
            }
            Effect::Preview { path } => {
                let file = FileQuery { path: path.clone() }.execute(&self.engine)?;
                Event::Previewed {
                    text: file.text,
                    highlights: file.highlights,
                    symbols: file.symbols,
                    path,
                }
            }
            Effect::Commit { intent } => {
                let engine = &self.engine;
                let applied = engine
                    .run(intent.clone().into_request(true))?
                    .into_applied()?;
                let id = applied.history_id();
                let answer: Answer = applied.into_inner().into();
                let report = Document::of(&answer);
                Event::Applied { id, intent, report }
            }
            Effect::History => Event::History(Ledger::new(&self.engine).history()?.entries),
            Effect::Undo => Event::Undone(Ledger::new(&self.engine).undo()?.undone),
            // Planned above; the loop runs `Edit` itself; `Touched` is
            // handled before anything is answered.
            Effect::Plan { .. } | Effect::Edit { .. } | Effect::Touched => {
                return Err(Failure::Unsupported("not a worker effect"));
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use vvv_engine::{BatchIntent, Languages, MemoryVfs, Workspace};

    #[test]
    fn a_batch_plan_is_rejected_as_a_user_visible_outcome() {
        let (outbox, inbox) = mpsc::channel();
        let runner = Runner {
            engine: Engine::new(
                Workspace::new("/ws", Arc::new(MemoryVfs::new())),
                Languages::new(),
            ),
            outbox,
        };
        runner.run(Effect::Plan {
            generation: 7,
            intent: Intent::Batch(BatchIntent::new([])),
            debounce: false,
        });
        assert!(
            matches!(inbox.recv().unwrap(), Event::PlanFailed { generation: 7, message }
            if message == "the picker plans one command at a time; use `vvv batch`")
        );
    }
    #[test]
    fn an_apply_event_carries_the_committed_history_id() {
        let (outbox, inbox) = mpsc::channel();
        let runner = Runner {
            engine: Engine::new(
                Workspace::new("/ws", Arc::new(MemoryVfs::new())),
                Languages::new(),
            ),
            outbox,
        };
        runner.run(Effect::Commit {
            intent: Intent::Batch(BatchIntent::new([])),
        });
        assert!(matches!(
            inbox.recv().unwrap(),
            Event::Applied { id: 1, .. }
        ));
        assert_eq!(
            Ledger::new(&runner.engine).history().unwrap().entries[0].id,
            1
        );
    }
}
