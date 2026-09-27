//! The runtime: what holds the graph and runs commands against it.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use vvv_core::{LanguageRegistry, Oracle};

use crate::command::{Command, Context};
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
    /// tree afresh for every command unless [`Engine::with_retention`] says
    /// otherwise.
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
        }
    }

    /// Keep files and facts between commands ([`Retention::Session`]) or
    /// read the workspace afresh each time (the default).
    pub fn with_retention(self, retention: Retention) -> Self {
        Self { retention, ..self }
    }

    /// Run one operation. Only graph-dependent capabilities refresh the tree.
    pub fn run<C: Command>(&self, command: C) -> Result<C::Output, EngineError> {
        let _operation = self.operation();
        let mut cx = Context::new(self);
        command.run(&mut cx)
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
    }

    /// The workspace root, absolute.
    pub fn root(&self) -> &Path {
        self.workspace.root()
    }

    /// The languages this engine understands.
    pub fn language_ids(&self) -> Vec<LanguageId> {
        self.languages.iter().map(|l| l.id().clone()).collect()
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
        graph.refresh(self.retention)?;
        Ok(graph)
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
