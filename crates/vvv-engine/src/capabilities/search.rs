//! Structural and symbolic search: request, result, execution, and report.
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{Files, Mark};
use crate::report::lines as l;
use crate::report::{Block, Document};
use crate::{EngineError, Match, Skipped};
use serde::{Deserialize, Serialize};
use vvv_core::Query;

/// A structural or symbolic search with a typed engine answer.
/// The wrapped query retains the existing wire shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SearchQuery(pub vvv_core::Query);

/// `vvv search`: what was found, and which languages could not be asked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Search {
    pub query: Query,
    pub matches: Vec<Match>,
    /// Languages whose grammar could not compile the query; their files
    /// were not searched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
}

impl From<vvv_core::Query> for SearchQuery {
    fn from(query: vvv_core::Query) -> Self {
        Self(query)
    }
}

/// Structural or symbolic search across the workspace: only files spelling
/// the query's literal words are parsed.
impl SearchQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<Search, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph)
    }

    pub(crate) fn execute_in(self, graph: &mut crate::graph::Graph) -> Result<Search, EngineError> {
        self.0.check()?;
        graph.search(&self.0)
    }
}

impl Document {
    pub fn search(matches: &[Match], skipped: &[Skipped]) -> Self {
        let mut report = Self::new();
        report.block_body(Block::Matches(matches.to_vec()));
        if matches.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no matches"),
            ));
        } else {
            report.block_note(Block::Summary(Self::search_summary(matches)));
        }
        for skipped in skipped {
            report.warning(l::SkippedLine::new(skipped).line());
        }
        report
    }

    /// `● 1  ○ 59   13 files`: what the search found.
    fn search_summary(matches: &[Match]) -> Line {
        let (declarations, uses) = l::Sections::split(matches);
        let mut counts: Vec<Line> = Vec::new();
        if !declarations.is_empty() {
            counts.push(
                Line::mark(Mark::Declaration)
                    .and(Role::Plain, " ")
                    .and(Role::Plain, declarations.len().to_string()),
            );
        }
        if !uses.is_empty() {
            counts.push(
                Line::of(Role::Dim, "○")
                    .and(Role::Plain, " ")
                    .and(Role::Plain, uses.len().to_string()),
            );
        }
        Self::join(counts, "  ").and(Role::Plain, "  ").and(
            Role::Dim,
            Files::among(matches.iter().map(|m| m.path.as_path())).to_string(),
        )
    }
}
