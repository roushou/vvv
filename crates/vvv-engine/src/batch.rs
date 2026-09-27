//! `batch`: several intents as one plan. Each is planned against the tree
//! as the previous one leaves it (an overlay the real files never see), and
//! all are applied in order as one transaction with one receipt — one
//! preview, one apply, one undo.

use crate::protocol::vocabulary::IntentLine;
use crate::report::{Block, Document, MoveCounts};
use serde::{Deserialize, Serialize};

use crate::{
    EngineError, FileChange, FilePreview, Intent, Mutation, MutationAnswer, Notice, Planned,
    Receipt, VfsError,
};

/// Several intents planned in sequence, each against the state the previous
/// one leaves, and applied as one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchIntent {
    pub intents: Vec<Intent>,
}

impl BatchIntent {
    pub fn new(intents: impl IntoIterator<Item = Intent>) -> Self {
        Self {
            intents: intents.into_iter().collect(),
        }
    }
}

/// `vvv batch`: several intents planned in sequence and applied as one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub intents: Vec<Intent>,
    /// Preview or successful application with its history entry.
    #[serde(flatten)]
    pub state: crate::MutationState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
    /// Every file any step touches, before the first step against after the
    /// last. Edits are not listed per file: they belong to the steps, each in
    /// the coordinates of the state before it.
    pub files: Vec<FileChange>,
}

impl Mutation for Batch {
    fn into_mutation(self) -> MutationAnswer {
        MutationAnswer::Batch(self)
    }
    fn applied(&mut self, id: u64) {
        self.state = crate::MutationState::Applied { history_id: id };
    }
}

/// Plan several intents as one. Each is planned against a staging copy of
/// the workspace onto which the previous steps have been applied, so a
/// rename may follow a move of the file it touches. Nothing real is
/// written; see [`Apply`](crate::Apply).
impl BatchIntent {
    /// Plan without writing files.
    pub fn plan(self, engine: &crate::Engine) -> Result<Planned<Batch>, EngineError> {
        let _operation = engine.operation();
        self.plan_in(engine)
    }

    pub(crate) fn plan_in(self, engine: &crate::Engine) -> Result<Planned<Batch>, EngineError> {
        let workspace = engine.workspace();
        let (engine, staging) = engine.staged();
        let mut steps = Vec::new();
        let mut notices = Vec::new();
        let mut receipt: Option<Receipt> = None;
        for intent in &self.intents {
            let planned = engine
                .run(intent.clone().into_request(false))?
                .into_preview()?;
            notices.extend_from_slice(planned.notices());
            for plan in planned.into_plans() {
                let applied = plan.clone().apply(&staging)?;
                receipt = Some(match receipt.take() {
                    Some(so_far) => so_far.then(applied),
                    None => applied,
                });
                steps.push(plan);
            }
        }
        // The combined view: each touched file as it is now against how the
        // staging tree leaves it, at its final path.
        let preview = match &receipt {
            Some(receipt) => receipt
                .paths()
                .map(|path| {
                    let mut final_path = path.to_path_buf();
                    for (from, to) in receipt.moves() {
                        if final_path == *from {
                            final_path = to.clone();
                        }
                    }
                    let before = workspace.vfs().read(&workspace.absolute(path))?;
                    let after = staging.vfs().read(&staging.absolute(&final_path))?;
                    Ok(FilePreview {
                        path: path.into(),
                        moved_to: (final_path != path).then_some(final_path.into()),
                        before,
                        after,
                    })
                })
                .collect::<Result<_, VfsError>>()?,
            None => Vec::new(),
        };
        let files = FileChange::all(None, &preview);
        Ok(Planned::new(
            Intent::Batch(self.clone()),
            Batch {
                intents: self.intents,
                state: crate::MutationState::Preview,
                notices,
                files,
            },
            steps,
            preview,
        ))
    }
}

impl Document {
    pub(crate) fn batch(result: &Batch) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::Batch(BatchIntent {
            intents: result.intents.clone(),
        })));
        report.block_body(Block::Batch(result.intents.clone()));
        report.block_body(Block::Blank);
        // Steps compose, so no edit is one re-spelled path: every file is a hunk.
        let structural = report.moved(result.state, &result.files, &[], &result.notices);
        report.moved_summary(
            result.state,
            MoveCounts {
                respellings: 0,
                structural,
                notices: result.notices.len(),
                files: result.files.len(),
            },
        );
        report
    }
}
