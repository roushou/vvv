//! A command's answer with the plan that would make it true.

use std::ops::{Deref, DerefMut};

use super::{FilePreview, Plan};
use crate::change::Change;
use crate::{EngineError, FileChange, Workspace};

/// What a mutating command returns: the result as a preview — `applied` is
/// false, `files` shows what would change — and, kept beside it, the plan(s)
/// [`Apply`](crate::Apply) writes. The plan never leaves the
/// process: a result that crossed to a client has nothing to apply.
#[derive(Debug, Clone)]
pub struct Planned<T> {
    result: T,
    /// One plan, or a batch's steps, each bound to the state the previous
    /// one leaves.
    plans: Vec<Plan>,
    /// Each touched file before and after, at its final path.
    preview: Vec<FilePreview>,
}

impl<T> Planned<T> {
    /// Bind a change to the tree and preview it; `result` gets what the plan
    /// does not carry (notices, respellings) and the per-file changes to
    /// build the command's answer around.
    pub(crate) fn of(
        workspace: &Workspace,
        change: Change,
        result: impl FnOnce(crate::change::Bound, Vec<FileChange>) -> T,
    ) -> Result<Self, EngineError> {
        let mut bound = change.bind()?;
        let change_set = std::mem::take(&mut bound.change_set);
        let plan = Plan::new(change_set, workspace)?;
        let preview = plan.preview(workspace)?.files;
        let files = FileChange::all(Some(&plan), &preview);
        Ok(Self::new(result(bound, files), vec![plan], preview))
    }

    pub(crate) fn new(result: T, plans: Vec<Plan>, preview: Vec<FilePreview>) -> Self {
        Self {
            result,
            plans,
            preview,
        }
    }

    /// The result alone: what a preview prints.
    pub fn into_inner(self) -> T {
        self.result
    }

    /// The result transformed, the plans and preview untouched.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Planned<U> {
        Planned {
            result: f(self.result),
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

    pub(crate) fn into_parts(self) -> (T, Vec<Plan>) {
        (self.result, self.plans)
    }

    /// The plans alone: a batch's steps, or a command's one.
    pub(crate) fn into_plans(self) -> Vec<Plan> {
        self.plans
    }
}

impl<T> Deref for Planned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.result
    }
}

impl<T> DerefMut for Planned<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.result
    }
}
