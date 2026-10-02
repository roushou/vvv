//! Structural and symbolic search: request, result, execution, and report.
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{Files, Mark};
use crate::report::lines as l;
use crate::report::{Block, Document};
use crate::{EngineError, Match, Skipped};
use serde::{Deserialize, Serialize};
use vvv_core::Query;

/// A structural or symbolic search with a typed engine answer.
/// Plugin predicates remain separate from engine-owned workspace filters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SearchQuery {
    #[serde(flatten)]
    pub query: Query,
    #[serde(default, skip_serializing_if = "SearchScope::is_empty")]
    pub scope: SearchScope,
}

/// Explicit file/directory prefixes and owning package names or IDs.
/// Alternatives within each list are ORed; the two lists are intersected.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct SearchScope {
    pub paths: Vec<crate::RelPath>,
    pub packages: Vec<String>,
}
impl SearchScope {
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.packages.is_empty()
    }
    pub fn validate(&self) -> Result<(), EngineError> {
        use std::path::Component;
        if self.paths.iter().any(|p| {
            p.as_str().contains(['\\', ':', '\0'])
                || p.components()
                    .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        }) || self.packages.iter().any(|p| p.is_empty())
        {
            return Err(EngineError::InvalidSearchScope);
        }
        Ok(())
    }
    pub(crate) fn includes_path(&self, path: &std::path::Path) -> bool {
        self.paths.is_empty()
            || self.paths.iter().any(|prefix| {
                let normalized: std::path::PathBuf = prefix
                    .components()
                    .filter(|c| !matches!(c, std::path::Component::CurDir))
                    .collect();
                path.starts_with(normalized)
            })
    }
    pub(crate) fn includes_package(&self, package: Option<&vvv_core::Package>) -> bool {
        self.packages.is_empty()
            || package.is_some_and(|p| {
                self.packages
                    .iter()
                    .any(|filter| filter == p.id.as_str() || filter == &p.name)
            })
    }
}

/// `vvv search`: what was found, and which languages could not be asked.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Search {
    pub query: Query,
    #[serde(default, skip_serializing_if = "SearchScope::is_empty")]
    pub scope: SearchScope,
    pub matches: Vec<Match>,
    /// Languages whose grammar could not compile the query; their files
    /// were not searched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
}

impl From<vvv_core::Query> for SearchQuery {
    fn from(query: vvv_core::Query) -> Self {
        Self {
            query,
            scope: SearchScope::default(),
        }
    }
}

/// Structural or symbolic search across the workspace: only files spelling
/// the query's literal words are parsed.
impl SearchQuery {
    pub fn scoped(mut self, scope: SearchScope) -> Self {
        self.scope = scope;
        self
    }
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<Search, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph)
    }

    pub(crate) fn execute_in(self, graph: &mut crate::graph::Graph) -> Result<Search, EngineError> {
        self.query.check()?;
        self.scope.validate()?;
        graph.search_scoped(&self.query, &self.scope)
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
                Line::single(Role::Dim, "○")
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SearchPageQuery {
    pub query: Query,
    #[serde(default)]
    pub scope: SearchScope,
    #[serde(default)]
    pub page: crate::PageBudget,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename = "search")]
pub struct SearchPage {
    #[serde(default, skip_serializing_if = "SearchScope::is_empty")]
    pub scope: SearchScope,
    pub snapshot: crate::SnapshotId,
    pub query: Query,
    pub items: Vec<SearchPageItem>,
    pub skipped: Vec<Skipped>,
    pub total_items: usize,
    pub next_cursor: Option<crate::Cursor>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SearchPageItem {
    /// Absolute, one-based position in the complete search ordering.
    pub ordinal: usize,
    #[serde(flatten)]
    pub item: Match,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SearchSession {
    pub next: usize,
}
impl SearchPageQuery {
    pub fn execute(self, engine: &crate::Engine) -> Result<SearchPage, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &crate::Engine) -> Result<SearchPage, EngineError> {
        use crate::{graph::query_snapshot::QuerySnapshot, query_store::QueryData};
        self.page.validate()?;
        self.query.check()?;
        self.scope.validate()?;
        let (mut graph, snapshot) = QuerySnapshot::capture(engine)?;
        let search = graph.search_bounded(
            &self.query,
            &self.scope,
            crate::QueryLimits::default().max_query_bytes,
        )?;
        let mut session = super::pagination::PageSession::new(
            engine,
            snapshot,
            &(&self.query, &self.scope),
            QueryData::Search(search),
        );
        let result = SearchSession { next: 0 }.page(engine, &mut session, self.page);
        match session.finish(engine, result)? {
            crate::PageReply::Search(page) => Ok(page),
            _ => unreachable!("capability returns its own page"),
        }
    }
}
impl SearchSession {
    pub(crate) fn page(
        mut self,
        engine: &crate::Engine,
        session: &mut super::pagination::PageSession,
        budget: crate::PageBudget,
    ) -> Result<crate::PageReply, EngineError> {
        use crate::{
            PageReply,
            query_store::{Checkpoint, QueryData},
        };
        let QueryData::Search(search) = &session.root.data else {
            return Err(EngineError::InvalidCursor);
        };
        let mut page = SearchPage {
            scope: search.scope.clone(),
            snapshot: session.root.identity.clone(),
            query: search.query.clone(),
            items: vec![],
            skipped: search.skipped.clone(),
            total_items: search.matches.len(),
            next_cursor: None,
        };
        while self.next < search.matches.len() && page.items.len() < budget.max_items {
            page.items.push(SearchPageItem {
                ordinal: self.next + 1,
                item: search.matches[self.next].clone(),
            });
            let after = Self {
                next: self.next + 1,
            };
            page.next_cursor = (after.next < search.matches.len())
                .then(|| session.token(engine, &Checkpoint::Search(after.clone())));
            if let Err(EngineError::OutputLimit {
                max_bytes,
                required_bytes,
            }) = budget.check(&PageReply::Search(page.clone()))
            {
                if page.items.len() == 1 {
                    let m = &page.items[0].item;
                    return Err(EngineError::PageOutputLimit {
                        max_bytes,
                        required_bytes,
                        anchor: m.content.clone().map(|content| crate::SourceAnchor {
                            path: m.path.clone(),
                            content,
                            span: m.span,
                        }),
                    });
                }
                page.items.pop();
                break;
            }
            self = after;
        }
        let next = Checkpoint::Search(self.clone());
        page.next_cursor = (self.next < search.matches.len()).then(|| session.token(engine, &next));
        let reply = PageReply::Search(page);
        budget.check(&reply)?;
        if reply.next_cursor().is_some() {
            session.retain(next);
        }
        Ok(reply)
    }
}
impl Document {
    pub(crate) fn search_page(page: &SearchPage) -> Self {
        let mut doc = Self::new();
        for item in &page.items {
            doc.body([Line::single(Role::Plain, item.ordinal.to_string())
                .and(Role::Plain, " ")
                .and(Role::Path, item.item.path.to_string())
                .and(Role::Plain, " ")
                .and(Role::Plain, &item.item.text)]);
        }
        for skipped in &page.skipped {
            doc.warning(l::SkippedLine::new(skipped).line());
        }
        doc
    }
}
