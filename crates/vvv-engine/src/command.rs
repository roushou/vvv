//! An engine runs commands. A [`Command`] is a request as data — an intent,
//! a question — that knows how to answer itself against the tree; the
//! engine's part is the plumbing: bring the graph up to date, hand it over,
//! and for a mutation bind the answer to a plan. Nothing about a command
//! lives on the engine.

use std::sync::MutexGuard;

use crate::graph::Graph;
use crate::{Engine, EngineError, Retention, Workspace};

/// One request and its answer. Implemented by the protocol's intents and
/// queries, each in the module that holds its components.
pub trait Command {
    type Output;

    /// Answer against the tree the context holds. A mutation answers with a
    /// [`Planned`](crate::Planned) result; nothing is written here.
    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError>;
}

/// What a command runs against: the graph, brought up to date, and the
/// workspace it was built from.
pub struct Context<'a> {
    pub(crate) graph: MutexGuard<'a, Graph>,
    pub(crate) workspace: &'a Workspace,
    engine: &'a Engine,
}

impl<'a> Context<'a> {
    pub(crate) fn new(engine: &'a Engine, graph: MutexGuard<'a, Graph>) -> Self {
        Self {
            graph,
            workspace: engine.workspace(),
            engine,
        }
    }

    /// An engine over a staging copy of the workspace — an overlay the real
    /// files never see — reading it afresh: how a batch plans each step
    /// against the state the previous one leaves.
    pub(crate) fn staged(&self) -> (Engine, Workspace) {
        let staging = self.workspace.staged();
        let engine = Engine::new(staging.clone(), self.engine.languages().clone())
            .with_retention(Retention::PerCall);
        (engine, staging)
    }
}
