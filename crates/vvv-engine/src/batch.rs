//! `batch`: several intents as one plan. Each is planned against the tree
//! as the previous one leaves it (an overlay the real files never see), and
//! all are applied in order as one transaction with one receipt — one
//! preview, one apply, one undo.

use crate::command::{Command, Context};
use crate::{Batch, BatchIntent, EngineError, FileChange, FilePreview, Planned, Receipt, VfsError};

/// Plan several intents as one. Each is planned against a staging copy of
/// the workspace onto which the previous steps have been applied, so a
/// rename may follow a move of the file it touches. Nothing real is
/// written; see [`Apply`](crate::Apply).
impl Command for BatchIntent {
    type Output = Planned<Batch>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let (engine, staging) = cx.staged();
        let mut steps = Vec::new();
        let mut notices = Vec::new();
        let mut receipt: Option<Receipt> = None;
        for intent in &self.intents {
            let planned = engine.run(intent.clone())?;
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
                    let before = cx.workspace.vfs().read(&cx.workspace.absolute(path))?;
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
            Batch {
                intents: self.intents,
                applied: false,
                history_id: None,
                notices,
                files,
            },
            steps,
            preview,
        ))
    }
}
