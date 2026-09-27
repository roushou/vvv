//! A command's answer with the plan that would make it true.

use std::ops::Deref;

use super::{FilePreview, Plan};
use crate::change::Change;
use crate::{EngineError, FileChange, Intent, Mutation, MutationAnswer, Workspace};

/// What a mutating command returns: the result as a preview — `applied` is
/// false, `files` shows what would change — and, kept beside it, the plan(s)
/// [`Apply`](crate::Apply) writes. The plan never leaves the
/// process: a result that crossed to a client has nothing to apply.
/// Queries cannot carry executable plans:
///
/// ```compile_fail,E0277
/// use vvv_engine::{Planned, Search};
/// let _: Option<Planned<Search>> = None;
/// ```
///
/// A query cannot replace a planned mutation's result:
///
/// ```compile_fail,E0599
/// use vvv_engine::{Answer, History, Planned, Rename};
/// let substitute = |planned: Planned<Rename>| {
///     planned.map(|_| Answer::History(History { entries: Vec::new() }))
/// };
/// ```
/// Executable plans expose their result only immutably:
///
/// ```compile_fail,E0596
/// use vvv_engine::{Planned, Rename};
/// let change_description = |mut planned: Planned<Rename>| {
///     planned.intent.to.clear();
/// };
/// ```
#[derive(Debug, Clone)]
pub struct Planned<T: Mutation> {
    result: T,
    /// The operation captured when the executable plans were constructed.
    intent: Intent,
    /// One plan, or a batch's steps, each bound to the state the previous
    /// one leaves.
    plans: Vec<Plan>,
    /// Each touched file before and after, at its final path.
    preview: Vec<FilePreview>,
}

impl<T: Mutation> Planned<T> {
    /// Bind a change to the tree and preview it; `result` gets what the plan
    /// does not carry (notices, respellings) and the per-file changes to
    /// build the command's answer around.
    pub(crate) fn of(
        workspace: &Workspace,
        change: Change,
        intent: Intent,
        result: impl FnOnce(crate::change::Bound, Vec<FileChange>) -> T,
    ) -> Result<Self, EngineError> {
        let mut bound = change.bind()?;
        let change_set = std::mem::take(&mut bound.change_set);
        let plan = Plan::new(change_set);
        let preview = plan.preview(workspace)?.files;
        let files = FileChange::all(Some(&plan), &preview);
        Ok(Self::new(intent, result(bound, files), vec![plan], preview))
    }

    pub(crate) fn new(
        intent: Intent,
        result: T,
        plans: Vec<Plan>,
        preview: Vec<FilePreview>,
    ) -> Self {
        Self {
            result,
            intent,
            plans,
            preview,
        }
    }

    /// The result alone: what a preview prints.
    pub fn into_inner(self) -> T {
        self.result
    }

    /// Preserve this executable plan while wrapping its mutation payload.
    pub fn into_mutation(self) -> Planned<MutationAnswer> {
        Planned {
            result: self.result.into_mutation(),
            intent: self.intent,
            plans: self.plans,
            preview: self.preview,
        }
    }

    /// Each touched file before and after.
    pub fn preview(&self) -> &[FilePreview] {
        &self.preview
    }

    pub fn is_empty(&self) -> bool {
        self.plans.iter().all(Plan::is_empty)
    }

    pub(crate) fn into_parts(self) -> (Intent, T, Vec<Plan>) {
        (self.intent, self.result, self.plans)
    }

    /// The plans alone: a batch's steps, or a command's one.
    pub(crate) fn into_plans(self) -> Vec<Plan> {
        self.plans
    }
}

impl<T: Mutation> Deref for Planned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Apply, Engine, HistoryQuery, Languages, MemoryVfs, Rename, RenameIntent};
    use std::sync::Arc;

    #[test]
    fn history_uses_the_captured_intent_even_if_internal_presentation_data_changes() {
        let intent = RenameIntent::new("old", "new");
        let captured = Intent::Rename(intent.clone());
        let mut planned = Planned::new(
            captured.clone(),
            Rename {
                intent,
                applied: false,
                history_id: None,
                declarations: Vec::new(),
                occurrences: Vec::new(),
                files: Vec::new(),
            },
            Vec::new(),
            Vec::new(),
        );
        // Only owner-side code can do this. History must still use the plan's intent.
        planned.result.intent.to = "presentation only".into();
        let engine = Engine::new(
            Workspace::new("/ws", Arc::new(MemoryVfs::new())),
            Languages::new(),
        );
        engine.run(Apply(planned.into_mutation())).unwrap();
        assert_eq!(
            engine.run(HistoryQuery).unwrap().entries[0].intent,
            captured
        );
    }
}
