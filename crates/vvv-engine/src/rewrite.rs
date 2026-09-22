//! `rewrite`: one edit per selected match, the template expanded with the
//! match's captures.

use std::collections::BTreeMap;
use vvv_core::Edit;
use vvv_core::RelPath;

use crate::change::Change;
use crate::command::{Command, Context};
use crate::{EngineError, Planned, Rewrite, RewriteIntent, RewriteOf, SourceFile};

/// Plan a rewrite. Nothing is written; see [`Apply`](crate::Apply).
impl Command for RewriteIntent {
    type Output = Planned<Rewrite>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let matches = cx.graph.search(&self.query)?.matches;
        RewriteOf {
            intent: self,
            matches,
        }
        .run(cx)
    }
}

/// The rewrite an intent makes of matches already found — a caller that
/// keeps the matches (the picker, showing before and after) searches once.
/// The intent's selection still narrows them.
impl Command for RewriteOf {
    type Output = Planned<Rewrite>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let RewriteOf { intent, matches } = self;
        let intent = &intent;
        let matches = intent.selection.narrow(matches)?;
        let mut files: BTreeMap<RelPath, SourceFile> = BTreeMap::new();
        let mut change = Change::new();
        for m in &matches {
            let file = match files.get(&m.path) {
                Some(file) => file,
                None => files
                    .entry(m.path.clone())
                    .or_insert(cx.workspace.load(&m.path)?),
            };
            let replacement = intent.template.expand(m, file.source()).map_err(|source| {
                EngineError::Template {
                    path: m.path.clone(),
                    line: m.start.line,
                    source,
                }
            })?;
            change.edit(&m.path, Edit::replace(m.span, replacement));
        }
        Planned::of(cx.workspace, change, |_, files| Rewrite {
            intent: intent.clone(),
            applied: false,
            history_id: None,
            files,
        })
    }
}
