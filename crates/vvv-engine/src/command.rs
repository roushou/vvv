//! An engine runs commands. A [`Command`] is a request as data — an intent,
//! a question — that knows how to answer itself against the tree; the
//! engine's part is the plumbing: bring the graph up to date, hand it over,
//! and for a mutation bind the answer to a plan. Nothing about a command
//! lives on the engine.

use crate::{Engine, EngineError, Workspace};

/// One request and its answer. Implemented by the protocol's intents and
/// queries, each in the module that holds its components.
pub trait Command {
    type Output;

    /// Answer against the tree the context holds. A mutation answers with a
    /// [`Planned`](crate::Planned) result; nothing is written here.
    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError>;
}

/// Temporary adapter context. Capabilities acquire the graph only when needed.
pub struct Context<'a> {
    pub(crate) workspace: &'a Workspace,
    pub(crate) engine: &'a Engine,
}

impl<'a> Context<'a> {
    pub(crate) fn new(engine: &'a Engine) -> Self {
        Self {
            workspace: engine.workspace(),
            engine,
        }
    }
}
