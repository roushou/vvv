//! `rename` and `references`: what both commands gather is the graph's
//! [`Evidence`](crate::graph::Evidence) for a name; `rename` then plans an
//! edit per chosen occurrence.

use vvv_core::Edit;

use crate::change::Change;
use crate::command::{Command, Context};
use crate::graph::Evidence;
use crate::{
    Confidence, EngineError, Match, Planned, References, ReferencesQuery, Rename, RenameIntent,
};

/// Plan a rename. Nothing is written; see [`Apply`](crate::Apply).
///
/// Occurrences are gathered only from the languages in which a matching
/// declaration exists, so a Rust `foo` never touches a TypeScript `foo`.
impl Command for RenameIntent {
    type Output = Planned<Rename>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let intent = &self;
        let graph = &mut *cx.graph;
        let Evidence {
            declarations,
            occurrences,
            ambiguous,
        } = graph.references(&intent.references())?;

        // What `Selection::All` means: never an occurrence of another
        // declaration; unresolved ones only when nothing else could be meant.
        let default: Vec<Match> = occurrences
            .iter()
            .filter(|o| match o.confidence {
                Confidence::Resolved => true,
                Confidence::Unresolved => !ambiguous.contains(&o.m.language),
                Confidence::Other => false,
            })
            .map(|o| o.m.clone())
            .collect();
        let chosen = if intent.selection.is_all() {
            default
        } else {
            occurrences.iter().map(|o| o.m.clone()).collect()
        };
        let mut change = Change::new();
        for m in intent.selection.narrow(chosen)? {
            change.edit(&m.path, Edit::replace(m.span, &intent.to));
        }
        Planned::of(cx.workspace, change, |_, files| Rename {
            intent: intent.clone(),
            applied: false,
            history_id: None,
            declarations,
            occurrences,
            files,
        })
    }
}

/// The declarations called `name` and every token spelling it, judged:
/// what `rename` acts on, answered without a plan.
impl Command for ReferencesQuery {
    type Output = References;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let query = &self;
        let graph = &mut *cx.graph;
        let evidence = graph.references(query)?;
        Ok(References {
            name: query.name.clone(),
            declarations: evidence.declarations,
            occurrences: evidence.occurrences,
        })
    }
}
