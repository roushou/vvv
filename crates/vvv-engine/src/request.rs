//! A [`Request`] is a command like any other: it runs the command it names
//! against the same context and answers with the [`Answer`] of that name.
//! A mutation with `apply` runs the intent, then [`Apply`] on what it
//! planned — one request, one undo — as the CLI's `--apply` does.
//!
//! An [`Intent`] is a command too: it plans to a [`MutationAnswer`], so a
//! caller that holds one never has to match its variants.

use crate::command::{Command, Context};
use crate::{
    Answer, Apply, EngineError, HistoryQuery, Intent, MutationAnswer, Planned, Request, UndoLast,
};

impl Command for Request {
    type Output = Answer;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        Ok(match self {
            Self::Search(q) => Answer::Search(q.run(cx)?),
            Self::Outline(q) => Answer::Outline(q.run(cx)?),
            Self::References(q) => Answer::References(q.run(cx)?),
            Self::Where(q) => Answer::Where(q.run(cx)?),
            Self::Deps(q) => Answer::Deps(q.run(cx)?),
            Self::Explain(q) => Answer::Explain(q.run(cx)?),
            Self::Surface(q) => Answer::Surface(q.run(cx)?),
            Self::Impact(q) => Answer::Impact(q.run(cx)?),
            Self::Dead(q) => Answer::Dead(q.run(cx)?),
            Self::Imports(q) => Answer::Imports(q.run(cx)?),
            Self::File(q) => Answer::File(q.run(cx)?),
            Self::Rewrite { intent, apply } => Self::mutate(Intent::Rewrite(intent), apply, cx)?,
            Self::Rename { intent, apply } => Self::mutate(Intent::Rename(intent), apply, cx)?,
            Self::Move { intent, apply } => Self::mutate(Intent::Move(intent), apply, cx)?,
            Self::MoveSymbol { intent, apply } => {
                Self::mutate(Intent::MoveSymbol(intent), apply, cx)?
            }
            Self::Batch { intent, apply } => Self::mutate(Intent::Batch(intent), apply, cx)?,
            Self::History => Answer::History(HistoryQuery.run(cx)?),
            Self::Undo => Answer::Undo(UndoLast.run(cx)?),
        })
    }
}

impl Request {
    /// Plan a mutating intent and, when asked, write it: the one place
    /// `--apply`, `serve`'s `apply` and a batch step agree on what applying
    /// means.
    fn mutate(intent: Intent, apply: bool, cx: &mut Context<'_>) -> Result<Answer, EngineError> {
        let planned = intent.run(cx)?;
        if apply {
            Apply(planned)
                .run(cx)
                .map(|applied| applied.into_inner().into())
        } else {
            Ok(planned.into_inner().into())
        }
    }
}

/// Plan any intent to the answer it will print. A caller that holds an
/// `Intent` — a batch step, the picker, `serve` — asks once, without
/// matching the variant.
impl Command for Intent {
    type Output = Planned<MutationAnswer>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        Ok(match self {
            Self::Rewrite(i) => i.run(cx)?.into_mutation(),
            Self::Rename(i) => i.run(cx)?.into_mutation(),
            Self::Move(i) => i.run(cx)?.into_mutation(),
            Self::MoveSymbol(i) => i.run(cx)?.into_mutation(),
            Self::Batch(i) => i.run(cx)?.into_mutation(),
        })
    }
}
