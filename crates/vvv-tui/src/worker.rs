//! The engine on its own thread. Effects go in, events come out; the UI
//! thread never blocks on a search or a plan. Consecutive searches (or
//! plans) are coalesced so a burst of keystrokes runs the last one only.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use vvv_engine::report::Document;
use vvv_engine::{
    Answer, Apply, Engine, EngineError, FileQuery, Intent, Ledger, MutationAnswer, SearchQuery,
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
            let mut runner = Runner {
                engine,
                outbox,
                last_definition: None,
                review: None,
            };
            while let Ok(effect) = inbox.recv() {
                let mut pending = Pending::default();
                pending.push(effect);
                while let Some(effect) = pending.next(&inbox) {
                    runner.run(effect);
                }
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

/// Pending preview bursts can contain both context and definition requests.
/// Other work is a barrier: explicit queries and mutations retain their order.
#[derive(Default)]
struct Pending {
    effects: Vec<Effect>,
}

impl Pending {
    /// Re-check the inbox between operations: a slow read must not make the
    /// worker execute every superseded preview from its original batch.
    fn next(&mut self, inbox: &Receiver<Effect>) -> Option<Effect> {
        while let Ok(next) = inbox.try_recv() {
            self.push(next);
        }
        (!self.effects.is_empty()).then(|| self.effects.remove(0))
    }

    fn push(&mut self, next: Effect) {
        let preview = matches!(next, Effect::Definition { .. } | Effect::Preview { .. });
        if preview {
            let start = self
                .effects
                .iter()
                .rposition(|e| !matches!(e, Effect::Definition { .. } | Effect::Preview { .. }))
                .map_or(0, |i| i + 1);
            if let Some(index) = (start..self.effects.len()).find(|&i| {
                std::mem::discriminant(&self.effects[i]) == std::mem::discriminant(&next)
            }) {
                self.effects.remove(index);
            }
        } else if matches!(
            (self.effects.last(), &next),
            (Some(Effect::Search { .. }), Effect::Search { .. })
                | (Some(Effect::Plan { .. }), Effect::Plan { .. })
        ) {
            self.effects.pop();
        }
        self.effects.push(next);
    }
}

struct Runner {
    engine: Engine,
    outbox: Sender<Event>,
    last_definition: Option<(u64, vvv_engine::NavigationQuery)>,
    review: Option<Review>,
}

/// The executable plan stays on the worker; events carry its display data only.
struct Review {
    generation: u64,
    intent: Intent,
    plan: vvv_engine::Planned<MutationAnswer>,
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

impl Failure {
    fn problem(self, retry: Effect) -> crate::problem::Problem {
        let failure = match self {
            Self::Engine(error) => vvv_engine::Failure::from(&error),
            Self::Unsupported(what) => {
                vvv_engine::Failure::new(vvv_engine::ErrorCode::BadRequest, what)
            }
        };
        crate::problem::Problem::new(failure, Some(retry))
    }
}

impl Runner {
    fn run(&mut self, effect: Effect) {
        if matches!(effect, Effect::Touched) {
            self.last_definition = None;
            self.review = None;
            self.engine.touched();
            return;
        }
        let retry = effect.clone();
        let generation = match &effect {
            Effect::WorkspaceFiles { generation }
            | Effect::Search { generation, .. }
            | Effect::Query { generation, .. } => Some(*generation),
            _ => None,
        };
        let event = match effect {
            Effect::Definition { ticket, query } => {
                if self.last_definition.as_ref() == Some(&(ticket, query.clone())) {
                    return;
                }
                self.last_definition = Some((ticket, query.clone()));
                let reply = query
                    .clone()
                    .execute(&self.engine)
                    .map_err(|e| vvv_engine::Failure::from(&e));
                Event::DefinitionResolved {
                    ticket,
                    query,
                    reply,
                }
            }
            Effect::Follow { ticket, query } => {
                let reply = query
                    .clone()
                    .execute(&self.engine)
                    .map_err(|e| vvv_engine::Failure::from(&e));
                Event::Followed {
                    ticket,
                    query,
                    reply,
                }
            }
            // A plan that cannot be made is an answer, not a failure.
            Effect::Plan {
                generation, intent, ..
            } => match self.plan(generation, intent) {
                Ok(planned) => Event::Planned {
                    generation,
                    planned,
                },
                Err(error) => Event::PlanFailed {
                    generation,
                    problem: Box::new(error.problem(retry)),
                },
            },
            other => match self.execute(other) {
                Ok(event) => event,
                Err(error) => Event::Failed {
                    generation,
                    problem: Box::new(error.problem(retry)),
                },
            },
        };
        let _ = self.outbox.send(event);
    }

    fn plan(&mut self, generation: u64, intent: Intent) -> Result<Planned, Failure> {
        self.review = None;
        let plan = self
            .engine
            .run(intent.clone().into_request(false))?
            .into_preview()?;
        let display = match &*plan {
            MutationAnswer::Rename(r) => Planned::Rename {
                files: r.files.clone(),
                declarations: r.declarations.clone(),
                occurrences: r.occurrences.clone(),
            },
            MutationAnswer::Move(mv) => Planned::Move {
                files: mv.files.clone(),
                intent: intent.clone(),
                respellings: mv.respellings.clone(),
                notices: mv.notices.clone(),
            },
            MutationAnswer::MoveSymbol(mv) => Planned::Move {
                files: mv.files.clone(),
                intent: intent.clone(),
                respellings: mv.respellings.clone(),
                notices: mv.notices.clone(),
            },
            MutationAnswer::Rewrite(rw) => Planned::Rewrite {
                files: rw.files.clone(),
            },
            MutationAnswer::Batch(_) => {
                return Err(Failure::Unsupported(
                    "the picker plans one command at a time; use `vvv batch`",
                ));
            }
        };
        self.review = Some(Review {
            generation,
            intent,
            plan,
        });
        Ok(display)
    }

    fn execute(&mut self, effect: Effect) -> Result<Event, Failure> {
        Ok(match effect {
            Effect::WorkspaceFiles { generation } => Event::WorkspaceFiles {
                generation,
                paths: vvv_engine::WorkspaceFilesQuery::default()
                    .execute(&self.engine)?
                    .paths,
            },
            Effect::Search {
                generation,
                query,
                scope,
            } => {
                let search = SearchQuery::from(query)
                    .scoped(scope)
                    .execute(&self.engine)?;
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
                    identifiers: file.identifiers,
                    path,
                }
            }
            Effect::Commit { generation, .. } => {
                if self
                    .review
                    .as_ref()
                    .is_none_or(|review| review.generation != generation)
                {
                    return Err(EngineError::StalePlan.into());
                }
                let review = self.review.take().expect("review generation checked");
                let applied = Apply(review.plan).apply(&self.engine)?;
                let id = applied.history_id();
                let answer: Answer = applied.into_inner().into();
                Event::Applied {
                    id,
                    intent: review.intent,
                    report: Document::of(&answer),
                }
            }
            Effect::History => Event::History(Ledger::new(&self.engine).history()?.entries),
            Effect::Undo => Event::Undone(Ledger::new(&self.engine).undo()?.undone),
            // Planned above; the loop runs `Edit` itself; `Touched` is
            // handled before anything is answered.
            Effect::Plan { .. }
            | Effect::Definition { .. }
            | Effect::Follow { .. }
            | Effect::Edit { .. }
            | Effect::Touched => {
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

    struct Fixture {
        runner: Runner,
        inbox: Receiver<Event>,
        intent: Intent,
    }

    impl Fixture {
        fn new() -> Self {
            let (outbox, inbox) = mpsc::channel();
            Self {
                runner: Runner {
                    engine: Engine::new(
                        Workspace::new("/ws", Arc::new(MemoryVfs::new())),
                        Languages::new(),
                    ),
                    outbox,
                    last_definition: None,
                    review: None,
                },
                inbox,
                intent: Intent::Rewrite(vvv_engine::RewriteIntent::new(
                    vvv_engine::Query::pattern("absent"),
                    "replacement",
                )),
            }
        }

        fn plan(&mut self, generation: u64) {
            self.runner.run(Effect::Plan {
                generation,
                intent: self.intent.clone(),
                debounce: false,
            });
            assert!(
                matches!(self.inbox.recv().unwrap(), Event::Planned { generation: actual, .. } if actual == generation)
            );
        }

        fn commit(&mut self, generation: u64) -> Event {
            self.runner.run(Effect::Commit {
                generation,
                intent: self.intent.clone(),
            });
            self.inbox.recv().unwrap()
        }
    }

    #[test]
    fn obsolete_generations_cannot_apply_or_consume_the_latest_review() {
        let mut fixture = Fixture::new();
        fixture.plan(7);
        fixture.plan(8);
        assert!(matches!(fixture.commit(7), Event::Failed { problem, .. }
            if problem.failure.code == vvv_engine::ErrorCode::Stale));
        assert!(
            Ledger::new(&fixture.runner.engine)
                .history()
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(matches!(fixture.commit(8), Event::Applied { id: 1, .. }));
    }

    #[test]
    fn failed_replanning_and_external_changes_discard_the_executable_review() {
        for touched in [false, true] {
            let mut fixture = Fixture::new();
            fixture.plan(7);
            if touched {
                fixture.runner.run(Effect::Touched);
            } else {
                fixture.runner.run(Effect::Plan {
                    generation: 8,
                    intent: Intent::Batch(BatchIntent::new([])),
                    debounce: false,
                });
                assert!(matches!(
                    fixture.inbox.recv().unwrap(),
                    Event::PlanFailed { generation: 8, .. }
                ));
            }
            assert!(matches!(fixture.commit(7), Event::Failed { problem, .. }
                if problem.failure.code == vvv_engine::ErrorCode::Stale));
            assert!(
                Ledger::new(&fixture.runner.engine)
                    .history()
                    .unwrap()
                    .entries
                    .is_empty()
            );
        }
    }

    #[test]
    fn a_batch_plan_is_rejected_as_a_user_visible_outcome() {
        let (outbox, inbox) = mpsc::channel();
        let mut runner = Runner {
            engine: Engine::new(
                Workspace::new("/ws", Arc::new(MemoryVfs::new())),
                Languages::new(),
            ),
            outbox,
            last_definition: None,
            review: None,
        };
        runner.run(Effect::Plan {
            generation: 7,
            intent: Intent::Batch(BatchIntent::new([])),
            debounce: false,
        });
        assert!(
            matches!(inbox.recv().unwrap(), Event::PlanFailed { generation: 7, problem }
            if problem.message() == "the picker plans one command at a time; use `vvv batch`")
        );
    }
    #[test]
    fn an_apply_event_carries_the_committed_history_id() {
        let (outbox, inbox) = mpsc::channel();
        let mut runner = Runner {
            engine: Engine::new(
                Workspace::new("/ws", Arc::new(MemoryVfs::new())),
                Languages::new(),
            ),
            outbox,
            last_definition: None,
            review: None,
        };
        let reviewed = Intent::Rewrite(vvv_engine::RewriteIntent::new(
            vvv_engine::Query::pattern("absent"),
            "replacement",
        ));
        runner.run(Effect::Plan {
            generation: 7,
            intent: reviewed.clone(),
            debounce: false,
        });
        assert!(matches!(
            inbox.recv().unwrap(),
            Event::Planned { generation: 7, .. }
        ));
        // Commit metadata cannot substitute another operation for the reviewed plan.
        runner.run(Effect::Commit {
            generation: 7,
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
        assert_eq!(
            Ledger::new(&runner.engine).history().unwrap().entries[0].intent,
            reviewed
        );
        runner.run(Effect::Commit {
            generation: 7,
            intent: reviewed,
        });
        assert!(
            matches!(inbox.recv().unwrap(), Event::Failed { problem, .. }
            if problem.failure.code == vvv_engine::ErrorCode::Stale)
        );
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    #[test]
    fn previews_arriving_during_work_replace_pending_reads_before_the_next_operation() {
        let (sender, inbox) = mpsc::channel();
        let mut pending = Pending::default();
        pending.push(Effect::Definition {
            ticket: 1,
            query: vvv_engine::NavigationQuery::at("old.rs", vvv_engine::Position::new(0, 0)),
        });
        pending.push(Effect::Preview {
            path: "old.rs".into(),
        });
        assert!(matches!(
            pending.next(&inbox),
            Some(Effect::Definition { ticket: 1, .. })
        ));
        // While that operation runs, a burst chooses a newer file.
        sender
            .send(Effect::Definition {
                ticket: 2,
                query: vvv_engine::NavigationQuery::at(
                    "latest.rs",
                    vvv_engine::Position::new(0, 0),
                ),
            })
            .unwrap();
        sender
            .send(Effect::Preview {
                path: "latest.rs".into(),
            })
            .unwrap();
        assert!(matches!(
            pending.next(&inbox),
            Some(Effect::Definition { ticket: 2, .. })
        ));
        assert!(
            matches!(pending.next(&inbox), Some(Effect::Preview { path }) if path.as_str() == "latest.rs")
        );
        assert!(pending.next(&inbox).is_none());
    }
    #[test]
    fn interleaved_preview_bursts_coalesce_but_explicit_queries_are_barriers() {
        let mut pending = Pending::default();
        for ticket in 0..3 {
            pending.push(Effect::Definition {
                ticket,
                query: vvv_engine::NavigationQuery::at(
                    "a.rs",
                    vvv_engine::Position::new(0, ticket as u32),
                ),
            });
            pending.push(Effect::Preview {
                path: "a.rs".into(),
            });
        }
        assert_eq!(pending.effects.len(), 2);
        assert!(matches!(
            pending.effects[0],
            Effect::Definition { ticket: 2, .. }
        ));
        pending.push(Effect::Query {
            generation: 1,
            request: vvv_engine::Request::History,
        });
        pending.push(Effect::Definition {
            ticket: 3,
            query: vvv_engine::NavigationQuery::at("a.rs", vvv_engine::Position::new(0, 3)),
        });
        assert_eq!(pending.effects.len(), 4);
        for ticket in [4, 5] {
            pending.push(Effect::Follow {
                ticket,
                query: vvv_engine::NavigationQuery::at("a.rs", vvv_engine::Position::new(0, 3)),
            });
        }
        assert_eq!(pending.effects.len(), 6);
        assert!(matches!(
            pending.effects[4],
            Effect::Follow { ticket: 4, .. }
        ));
        assert!(matches!(
            pending.effects[5],
            Effect::Follow { ticket: 5, .. }
        ));
    }
}
