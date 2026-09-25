//! A [`Request`] is a command like any other: it runs the command it names
//! against the same context and answers with the [`Answer`] of that name.
//! A mutation with `apply` runs the intent, then [`Apply`] on what it
//! planned — one request, one undo — as the CLI's `--apply` does.
//!
//! An [`Intent`] is a command too: it plans to the wire [`Answer`], so a
//! caller that holds one never has to match its variants.

use crate::command::{Command, Context};
use crate::{
    Answer, Apply, EngineError, HistoryQuery, Intent, Mutation, Planned, Request, UndoLast,
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
            Apply(planned).run(cx)
        } else {
            Ok(planned.into_inner())
        }
    }
}

/// Plan any intent to the answer it will print. A caller that holds an
/// `Intent` — a batch step, the picker, `serve` — asks once, without
/// matching the variant.
impl Command for Intent {
    type Output = Planned<Answer>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        Ok(match self {
            Self::Rewrite(i) => i.run(cx)?.map(Answer::Rewrite),
            Self::Rename(i) => i.run(cx)?.map(Answer::Rename),
            Self::Move(i) => i.run(cx)?.map(Answer::Move),
            Self::MoveSymbol(i) => i.run(cx)?.map(Answer::MoveSymbol),
            Self::Batch(i) => i.run(cx)?.map(Answer::Batch),
        })
    }
}

/// A written answer records the intent it wrote and the history entry.
///
/// Only a mutating answer reaches [`Apply`]: a `Planned<Answer>` is built
/// from a mutating intent, so the query variants are unreachable here.
impl Mutation for Answer {
    fn intent(&self) -> Intent {
        match self {
            Self::Rewrite(r) => r.intent(),
            Self::Rename(r) => r.intent(),
            Self::Move(r) => r.intent(),
            Self::MoveSymbol(r) => r.intent(),
            Self::Batch(b) => b.intent(),
            _ => unreachable!("a query answer is never written"),
        }
    }

    fn applied(&mut self, id: u64) {
        match self {
            Self::Rewrite(r) => r.applied(id),
            Self::Rename(r) => r.applied(id),
            Self::Move(r) => r.applied(id),
            Self::MoveSymbol(r) => r.applied(id),
            Self::Batch(b) => b.applied(id),
            _ => {}
        }
    }
}
