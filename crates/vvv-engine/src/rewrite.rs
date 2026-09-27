//! `rewrite`: one edit per selected match, the template expanded with the
//! match's captures from the same immutable source snapshot.

use std::collections::{BTreeMap, btree_map::Entry};
use vvv_core::{Edit, RelPath};

use crate::change::Change;
use crate::command::{Command, Context};
use crate::graph::Graph;
use crate::{Candidate, EngineError, Match, Planned, Rewrite, RewriteIntent, RewriteOf, Workspace};

/// Plan a rewrite. Nothing is written; see [`Apply`](crate::Apply).
impl RewriteIntent {
    /// Plan without writing files.
    pub fn plan(self, engine: &crate::Engine) -> Result<Planned<Rewrite>, EngineError> {
        engine.run(self)
    }

    pub(crate) fn plan_in(
        self,
        graph: &mut crate::graph::Graph,
        workspace: &crate::Workspace,
    ) -> Result<Planned<Rewrite>, EngineError> {
        let matches = graph.search(&self.query)?.matches;
        RewriteMatches::new(self, matches, graph)?.plan(workspace)
    }
}

// Temporary adapter while callers migrate to typed capability methods.
impl Command for RewriteIntent {
    type Output = Planned<Rewrite>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        self.plan_in(&mut cx.graph, cx.workspace)
    }
}

/// Retained matches are revalidated against their candidate snapshots before
/// expansion. The intent's selection still narrows them.
impl RewriteOf {
    /// Plan without writing files.
    pub fn plan(self, engine: &crate::Engine) -> Result<Planned<Rewrite>, EngineError> {
        engine.run(self)
    }

    pub(crate) fn plan_in(
        self,
        graph: &mut crate::graph::Graph,
        workspace: &crate::Workspace,
    ) -> Result<Planned<Rewrite>, EngineError> {
        let matched = RewriteMatches::new(self.intent, self.matches, graph)?;
        for file in matched.files.values() {
            file.candidate
                .validate_matches(&matched.intent.query, &file.matches)?;
        }
        matched.plan(workspace)
    }
}

// Temporary adapter while callers migrate to typed capability methods.
impl Command for RewriteOf {
    type Output = Planned<Rewrite>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        self.plan_in(&mut cx.graph, cx.workspace)
    }
}

/// Selected matches and the immutable candidates whose text supplies their
/// captures and edit provenance.
struct RewriteMatches {
    intent: RewriteIntent,
    files: BTreeMap<RelPath, FileMatches>,
}

struct FileMatches {
    candidate: Candidate,
    matches: Vec<Match>,
}

impl RewriteMatches {
    fn new(intent: RewriteIntent, matches: Vec<Match>, graph: &Graph) -> Result<Self, EngineError> {
        let mut files: BTreeMap<RelPath, FileMatches> = BTreeMap::new();
        for m in intent.selection.narrow(matches)? {
            let file = match files.entry(m.path.clone()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(FileMatches {
                    candidate: graph.file(&m.path)?,
                    matches: Vec::new(),
                }),
            };
            file.matches.push(m);
        }
        Ok(Self { intent, files })
    }

    fn plan(self, workspace: &Workspace) -> Result<Planned<Rewrite>, EngineError> {
        let mut change = Change::new();
        for file in self.files.values() {
            file.edits(&self.intent, &mut change)?;
        }
        Planned::of(
            workspace,
            change,
            crate::Intent::Rewrite(self.intent.clone()),
            |_, files| Rewrite {
                intent: self.intent,
                state: crate::MutationState::Preview,
                files,
            },
        )
    }
}

impl FileMatches {
    fn edits(&self, intent: &RewriteIntent, change: &mut Change) -> Result<(), EngineError> {
        let file = self.candidate.file();
        for m in &self.matches {
            let replacement = intent.template.expand(m, file.source()).map_err(|source| {
                EngineError::Template {
                    path: m.path.clone(),
                    line: m.start.line,
                    source,
                }
            })?;
            change.edit(file.witness(), Edit::replace(m.span, replacement))?;
        }
        Ok(())
    }
}
