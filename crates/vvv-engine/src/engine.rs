//! The runtime: what holds the graph and runs commands against it.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use vvv_core::{LanguageRegistry, Oracle};

use crate::graph::{Graph, Retention};
use crate::{EngineError, LanguageId, Workspace};

#[derive(Clone)]
pub struct Engine {
    workspace: Workspace,
    languages: LanguageRegistry,
    retention: Retention,
    /// What a per-call graph is built with; a session's graph holds its own.
    oracle: Option<Arc<dyn Oracle>>,
    /// Shared by clones, so a session's worker and its owner see one graph.
    graph: Arc<Mutex<Graph>>,
    /// One complete operation, including planning, writes and recovery.
    operation: Arc<Mutex<()>>,
    /// Invalidation does not acquire or refresh the graph.
    dirty: Arc<AtomicBool>,
    queries: Arc<Mutex<crate::query_store::QueryStore>>,
    query_revision: Arc<AtomicU64>,
    cancellation: Option<crate::ReadCancellation>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("workspace", &self.workspace)
            .field("retention", &self.retention)
            .field("oracle", &self.oracle.is_some())
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// An engine over `workspace` that understands `languages`. Reads the
    /// tree afresh for every graph-dependent request unless
    /// [`Engine::with_retention`] says otherwise.
    pub fn new(workspace: Workspace, languages: LanguageRegistry) -> Self {
        let graph = Graph::new(workspace.clone(), languages.clone());
        Self {
            workspace,
            languages,
            retention: Retention::default(),
            oracle: None,
            graph: Arc::new(Mutex::new(graph)),
            operation: Arc::new(Mutex::new(())),
            dirty: Arc::new(AtomicBool::new(false)),
            queries: Arc::new(Mutex::new(crate::query_store::QueryStore::default())),
            query_revision: Arc::new(AtomicU64::new(0)),
            cancellation: None,
        }
    }

    /// Keep files and facts between commands ([`Retention::Session`]) or
    /// read the workspace afresh each time (the default).
    pub fn with_retention(self, retention: Retention) -> Self {
        Self { retention, ..self }
    }

    /// Run one request under operation exclusion. Acquire the graph only for
    /// capabilities that need source-tree facts; retain mutation plans until
    /// the caller applies them or converts the execution to a wire answer.
    /// A typed query goes through its capability method, not this dispatcher:
    ///
    /// ```compile_fail,E0308
    /// use vvv_engine::{Engine, Languages, MemoryVfs, Query, Workspace};
    /// use std::sync::Arc;
    /// let engine = Engine::new(Workspace::new("/ws", Arc::new(MemoryVfs::new())), Languages::new());
    /// engine.run(Query::pattern("foo"));
    /// ```
    pub fn run(&self, request: crate::Request) -> Result<Execution, EngineError> {
        let _operation = self.operation();
        self.check_read()?;
        let result = (|| {
            Ok(match request {
                #[cfg(feature = "schema")]
                crate::Request::Schema(query) => {
                    Execution::Completed(crate::Answer::Schema(query.execute()?))
                }
                crate::Request::SearchPage(query) => {
                    Execution::Completed(crate::Answer::SearchPage(query.execute_in(self)?))
                }
                crate::Request::ContextPage(query) => {
                    Execution::Completed(crate::Answer::ContextPage(query.execute_in(self)?))
                }
                crate::Request::Continue(query) => {
                    Execution::Completed(crate::Answer::Continue(query.execute_in(self)?))
                }
                crate::Request::Expand(query) => {
                    Execution::Completed(crate::Answer::Expand(query.execute_in(self)?))
                }
                crate::Request::Discover(query) => {
                    Execution::Completed(crate::Answer::Discover(query.execute(self)))
                }
                crate::Request::Context(query) => Execution::Completed(crate::Answer::Context(
                    query.execute_in(&mut *self.graph()?)?,
                )),
                crate::Request::Relationships(query) => {
                    Execution::Completed(crate::Answer::Relationships(query.execute_in(self)?))
                }
                crate::Request::Resolve(query) => Execution::Completed(crate::Answer::Resolve(
                    query.execute_in(&mut *self.graph()?)?,
                )),
                crate::Request::Navigate(query) => Execution::Completed(crate::Answer::Navigate(
                    query.execute_in(&mut *self.graph()?)?,
                )),
                crate::Request::Search(query) => Execution::Completed(crate::Answer::Search(
                    query.execute_in(&mut *self.graph()?)?,
                )),
                crate::Request::Outline(query) => Execution::Completed(crate::Answer::Outline(
                    query.execute_in(&mut *self.graph()?, self.workspace())?,
                )),
                crate::Request::References(query) => Execution::Completed(
                    crate::Answer::References(query.execute_in(&mut *self.graph()?)?),
                ),
                crate::Request::Where(query) => Execution::Completed(crate::Answer::Where(
                    query.execute_in(&mut *self.graph()?, self.workspace())?,
                )),
                crate::Request::Deps(query) => Execution::Completed(crate::Answer::Deps(
                    query.execute_in(&mut *self.graph()?, self.workspace())?,
                )),
                crate::Request::Explain(query) => Execution::Completed(crate::Answer::Explain(
                    query.execute_in(&mut *self.graph()?, self.workspace())?,
                )),
                crate::Request::Surface(query) => Execution::Completed(crate::Answer::Surface(
                    query.execute_in(&mut *self.graph()?)?,
                )),
                crate::Request::Impact(query) => Execution::Completed(crate::Answer::Impact(
                    query.execute_in(&mut *self.graph()?)?,
                )),
                crate::Request::Dead(query) => Execution::Completed(crate::Answer::Dead(
                    query.execute_in(&mut *self.graph()?)?,
                )),
                crate::Request::Imports(query) => Execution::Completed(crate::Answer::Imports(
                    query.execute_in(&mut *self.graph()?, self.workspace())?,
                )),
                crate::Request::File(query) => Execution::Completed(crate::Answer::File(
                    query.execute_in(self.workspace(), self.languages())?,
                )),
                crate::Request::Rename { intent, apply } => {
                    let planned = {
                        let mut graph = self.graph()?;
                        intent.plan_in(&mut graph, self.workspace())?
                    };
                    self.mutation(planned, apply)?
                }
                crate::Request::Move { intent, apply } => {
                    let planned = {
                        let mut graph = self.graph()?;
                        intent.plan_in(&mut graph, self.workspace())?
                    };
                    self.mutation(planned, apply)?
                }
                crate::Request::MoveSymbol { intent, apply } => {
                    let planned = {
                        let mut graph = self.graph()?;
                        intent.plan_in(&mut graph, self.workspace())?
                    };
                    self.mutation(planned, apply)?
                }
                crate::Request::Rewrite { intent, apply } => {
                    let planned = {
                        let mut graph = self.graph()?;
                        intent.plan_in(&mut graph, self.workspace())?
                    };
                    self.mutation(planned, apply)?
                }
                crate::Request::Batch { intent, apply } => {
                    self.mutation(intent.plan_in(self)?, apply)?
                }
                crate::Request::History => Execution::Completed(crate::Answer::History(
                    crate::Ledger::new(self).history_in()?,
                )),
                crate::Request::Undo => {
                    Execution::Completed(crate::Answer::Undo(crate::Ledger::new(self).undo_in()?))
                }
            })
        })();
        self.check_read()?;
        result
    }

    fn mutation<T: crate::Mutation>(
        &self,
        planned: crate::Planned<T>,
        apply: bool,
    ) -> Result<Execution, EngineError> {
        if apply {
            Ok(Execution::Applied(
                crate::Apply(planned).apply_in(self)?.into_mutation(),
            ))
        } else {
            Ok(Execution::Preview(planned.into_mutation()))
        }
    }

    /// Ask `oracle` about the tokens syntax cannot place — a build, a
    /// language server, an index the host has. The graph judges every token
    /// by imports first; the oracle is consulted only where that says `?`,
    /// and its answer counts only when it names a declaration the graph
    /// knows.
    pub fn with_oracle(mut self, oracle: Arc<dyn Oracle>) -> Self {
        {
            let _operation = self.operation();
            let mut graph = self
                .graph
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *graph = Graph::new(self.workspace.clone(), self.languages.clone())
                .with_oracle(oracle.clone());
        }
        self.oracle = Some(oracle);
        self
    }

    /// Tell a session the tree changed behind its back — an editor wrote,
    /// say — so its next command walks even within the trusted window.
    pub fn touched(&self) {
        self.dirty.store(true, Ordering::Release);
        self.query_revision.fetch_add(1, Ordering::AcqRel);
    }

    /// The workspace root, absolute.
    pub fn root(&self) -> &Path {
        self.workspace.root()
    }

    /// The languages this engine understands.
    pub fn language_ids(&self) -> Vec<LanguageId> {
        self.languages.iter().map(|l| l.id().clone()).collect()
    }

    pub(crate) fn queries(&self) -> MutexGuard<'_, crate::query_store::QueryStore> {
        self.queries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(crate) fn query_revision(&self) -> u64 {
        self.query_revision.load(Ordering::Acquire)
    }

    pub(crate) fn with_cancellation(&self, cancellation: crate::ReadCancellation) -> Self {
        Self {
            cancellation: Some(cancellation),
            ..self.clone()
        }
    }
    pub(crate) fn cancellation(&self) -> Option<crate::ReadCancellation> {
        self.cancellation.clone()
    }
    pub(crate) fn check_read(&self) -> Result<(), EngineError> {
        self.cancellation
            .as_ref()
            .map_or(Ok(()), crate::ReadCancellation::check)
    }
    pub(crate) fn publish_read<T>(
        &self,
        publish: impl FnOnce() -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        match &self.cancellation {
            Some(c) => c.complete(publish),
            None => publish(),
        }
    }

    pub(crate) fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub(crate) fn languages(&self) -> &LanguageRegistry {
        &self.languages
    }

    /// Shared by clones; acquire before the graph or transaction work.
    pub(crate) fn operation(&self) -> MutexGuard<'_, ()> {
        self.operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A fresh staging session for batch composition.
    pub(crate) fn staged(&self) -> (Engine, Workspace) {
        let staging = self.workspace.staged();
        let engine =
            Engine::new(staging.clone(), self.languages.clone()).with_retention(Retention::PerCall);
        (engine, staging)
    }

    /// The graph, brought up to date with the tree. Held for the command;
    /// per-file work happens on the candidates it hands out.
    pub(crate) fn graph(&self) -> Result<MutexGuard<'_, Graph>, EngineError> {
        let mut graph = self
            .graph
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.dirty.swap(false, Ordering::AcqRel) {
            graph.touched();
        }
        if self.retention == Retention::PerCall {
            let fresh = Graph::new(self.workspace.clone(), self.languages.clone());
            *graph = match &self.oracle {
                Some(oracle) => fresh.with_oracle(oracle.clone()),
                None => fresh,
            };
        }
        graph.cancellation = self.cancellation();
        graph.refresh(self.retention)?;
        Ok(graph)
    }
}

/// An in-process result. Only mutation previews carry executable plans.
#[derive(Debug)]
pub enum Execution {
    Completed(crate::Answer),
    Preview(crate::Planned<crate::MutationAnswer>),
    Applied(crate::Applied<crate::MutationAnswer>),
}

/// The kind of retained result requested by an in-process caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionKind {
    Completed,
    Preview,
    Applied,
}

impl std::fmt::Display for ExecutionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Completed => "completed",
            Self::Preview => "preview",
            Self::Applied => "applied",
        })
    }
}

impl Execution {
    pub fn kind(&self) -> ExecutionKind {
        match self {
            Self::Completed(_) => ExecutionKind::Completed,
            Self::Preview(_) => ExecutionKind::Preview,
            Self::Applied(_) => ExecutionKind::Applied,
        }
    }

    /// Consume the retained handle at a reporting or wire boundary.
    pub fn into_answer(self) -> crate::Answer {
        match self {
            Self::Completed(answer) => answer,
            Self::Preview(planned) => planned.into_inner().into(),
            Self::Applied(applied) => applied.into_inner().into(),
        }
    }

    pub fn into_preview(self) -> Result<crate::Planned<crate::MutationAnswer>, EngineError> {
        match self {
            Self::Preview(planned) => Ok(planned),
            other => Err(EngineError::ExecutionKind {
                expected: ExecutionKind::Preview,
                actual: other.kind(),
            }),
        }
    }

    pub fn into_applied(self) -> Result<crate::Applied<crate::MutationAnswer>, EngineError> {
        match self {
            Self::Applied(applied) => Ok(applied),
            other => Err(EngineError::ExecutionKind {
                expected: ExecutionKind::Applied,
                actual: other.kind(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_clones_exclude_whole_operations_even_without_a_graph() {
        let engine = Engine::new(
            Workspace::new("/ws", Arc::new(crate::MemoryVfs::new())),
            LanguageRegistry::new(),
        );
        let guard = engine.operation();
        let clone = engine.clone();
        let (started, ready) = std::sync::mpsc::channel();
        let (finished, result) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            finished.send(crate::Ledger::new(&clone).history()).unwrap();
        });
        ready.recv().unwrap();
        assert!(matches!(
            result.recv_timeout(std::time::Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(guard);
        assert!(result.recv().unwrap().unwrap().entries.is_empty());
        worker.join().unwrap();
    }
}
