//! `rewrite`: one edit per selected match, the template expanded with the
//! match's captures from the same immutable source snapshot.

use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{IntentLine, Mark, Plural};
use crate::report::{Block, Document};
use crate::{Intent, Mutation, Selection, Template};
use serde::{Deserialize, Serialize};

use std::collections::{BTreeMap, btree_map::Entry};
use vvv_core::{Edit, Query, RelPath};

use crate::change::Change;
use crate::graph::Graph;
use crate::{Candidate, EngineError, FileChange, Match, Planned, Workspace};

/// The rewrite `intent` makes of `matches` already found: a caller that
/// keeps the matches supplies its selection; selected matches are revalidated
/// against the source before their coordinates and captures are used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteOf {
    pub intent: RewriteIntent,
    pub matches: Vec<crate::Match>,
}

/// Replace every selected match of `query` with `template`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RewriteIntent {
    pub query: Query,
    pub template: Template,
    #[serde(default, skip_serializing_if = "Selection::is_all")]
    pub selection: Selection,
}

impl RewriteIntent {
    pub fn new(query: Query, template: impl Into<Template>) -> Self {
        Self {
            query,
            template: template.into(),
            selection: Selection::All,
        }
    }

    pub fn selecting(mut self, selection: Selection) -> Self {
        self.selection = selection;
        self
    }

    /// Plan a rewrite without writing files; see [`Apply`](crate::Apply).
    pub fn plan(self, engine: &crate::Engine) -> Result<Planned<Rewrite>, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.plan_in(&mut graph, engine.workspace())
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

/// `vvv rewrite`: one edit per selected match.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Rewrite {
    pub intent: RewriteIntent,
    /// Preview or successful application with its history entry.
    #[serde(flatten)]
    pub state: crate::MutationState,
    pub files: Vec<FileChange>,
}

impl Mutation for Rewrite {
    fn into_mutation(self) -> crate::MutationAnswer {
        crate::MutationAnswer::Rewrite(self)
    }

    fn applied(&mut self, id: u64) {
        self.state = crate::MutationState::Applied { history_id: id };
    }
}

/// Retained matches are revalidated against their candidate snapshots before
/// expansion. The intent's selection still narrows them.
impl RewriteOf {
    /// Plan without writing files.
    pub fn plan(self, engine: &crate::Engine) -> Result<Planned<Rewrite>, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.plan_in(&mut graph, engine.workspace())
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

impl Document {
    pub(crate) fn rewrite(result: &Rewrite) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::Rewrite(result.intent.clone())));
        report.block_body(Block::Changes {
            state: result.state,
            files: result.files.clone(),
        });
        let edits = Self::edits_in(&result.files);
        if edits == 0 {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no matches"),
            ));
            return report;
        }
        report.receipt(
            result.state,
            Line::mark(Mark::Rewrite)
                .and(Role::Plain, format!(" {edits}   "))
                .and(Role::Dim, Plural(result.files.len(), "file").to_string()),
        );
        report
    }
}
