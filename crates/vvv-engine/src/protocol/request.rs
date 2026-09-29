//! Every request as one value, and every answer as one: what a client sends
//! down a wire and reads back. `{"command": "rename", "name": "Config",
//! "to": "Settings", "apply": true}` is a [`Request`]; the reply's `result`
//! is the [`Answer`] of the same name.

use serde::{Deserialize, Serialize};
#[cfg(test)]
use vvv_core::Query;

use crate::capabilities::moves::{Move, MoveIntent, MoveSymbol, MoveSymbolIntent};
use crate::capabilities::rename::{Rename, RenameIntent};

use super::{
    Batch, BatchIntent, Dead, DeadQuery, Deps, DepsQuery, ExplainQuery, Explanation, File,
    FileQuery, History, Impact, ImpactQuery, ImportsQuery, ImportsReport, Locations,
    MutationAnswer, Notice, Outline, OutlineQuery, References, ReferencesQuery, Response, Rewrite,
    RewriteIntent, Search, Surface, SurfaceQuery, Undo, WhereQuery,
};

/// One request, tagged by `command`. A mutation carries the same fields as
/// its [`Intent`](super::Intent) plus `apply`: false previews, true writes
/// and records one undo — exactly what the CLI's `--apply` does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    DiscardPlan(crate::DiscardPlanQuery),
    ApplyPlan(crate::ApplyPlanQuery),
    ValidatePlan(crate::ValidatePlanQuery),
    InspectPlan(crate::InspectPlanQuery),
    PrepareRename(crate::PrepareRenameQuery),
    PrepareRewrite(crate::PrepareRewriteQuery),
    ReviewPlan(crate::ReviewPlanQuery),
    #[cfg(feature = "schema")]
    Schema(crate::SchemaQuery),
    SearchPage(crate::SearchPageQuery),
    ContextPage(crate::ContextPageQuery),
    Continue(crate::ContinueQuery),
    Expand(crate::ExpandQuery),
    Discover(crate::DiscoveryQuery),
    Context(crate::ContextQuery),
    Navigate(crate::NavigationQuery),
    Relationships(crate::RelationshipsQuery),
    Resolve(crate::ResolutionQuery),
    Search(crate::SearchQuery),
    Outline(OutlineQuery),
    References(ReferencesQuery),
    Where(WhereQuery),
    Deps(DepsQuery),
    Explain(ExplainQuery),
    Surface(SurfaceQuery),
    Impact(ImpactQuery),
    Dead(DeadQuery),
    Imports(ImportsQuery),
    File(FileQuery),
    Rewrite {
        #[serde(flatten)]
        intent: RewriteIntent,
        #[serde(default)]
        apply: bool,
    },
    Rename {
        #[serde(flatten)]
        intent: RenameIntent,
        #[serde(default)]
        apply: bool,
    },
    Move {
        #[serde(flatten)]
        intent: MoveIntent,
        #[serde(default)]
        apply: bool,
    },
    MoveSymbol {
        #[serde(flatten)]
        intent: MoveSymbolIntent,
        #[serde(default)]
        apply: bool,
    },
    Batch {
        #[serde(flatten)]
        intent: BatchIntent,
        #[serde(default)]
        apply: bool,
    },
    History,
    Undo,
}

impl Request {
    /// Whether running this request can write. The picker's hub asks only
    /// these; a mutation goes through an `Intent` and `Apply`.
    /// Reads and explicit checks can stop; applying a transaction cannot.
    pub fn is_cancellable(&self) -> bool {
        self.is_read_only() || matches!(self, Self::ValidatePlan(_))
    }
    pub fn is_read_only(&self) -> bool {
        self.command().is_read_only()
    }

    pub fn command(&self) -> super::Command {
        match self {
            Self::DiscardPlan(_) => super::Command::DiscardPlan,
            Self::ApplyPlan(_) => super::Command::ApplyPlan,
            Self::ValidatePlan(_) => super::Command::ValidatePlan,
            Self::InspectPlan(_) => super::Command::InspectPlan,
            Self::PrepareRename(_) => super::Command::PrepareRename,
            Self::PrepareRewrite(_) => super::Command::PrepareRewrite,
            Self::ReviewPlan(_) => super::Command::ReviewPlan,
            #[cfg(feature = "schema")]
            Self::Schema(_) => super::Command::Schema,
            Self::SearchPage(_) => super::Command::SearchPage,
            Self::ContextPage(_) => super::Command::ContextPage,
            Self::Continue(_) => super::Command::Continue,
            Self::Expand(_) => super::Command::Expand,
            Self::Discover(_) => super::Command::Discover,
            Self::Context(_) => super::Command::Context,
            Self::Navigate(_) => super::Command::Navigate,
            Self::Relationships(_) => super::Command::Relationships,
            Self::Resolve(_) => super::Command::Resolve,
            Self::Search(_) => super::Command::Search,
            Self::Outline(_) => super::Command::Outline,
            Self::References(_) => super::Command::References,
            Self::Where(_) => super::Command::Where,
            Self::Deps(_) => super::Command::Deps,
            Self::Explain(_) => super::Command::Explain,
            Self::Surface(_) => super::Command::Surface,
            Self::Impact(_) => super::Command::Impact,
            Self::Dead(_) => super::Command::Dead,
            Self::Imports(_) => super::Command::Imports,
            Self::File(_) => super::Command::File,
            Self::Rewrite { .. } => super::Command::Rewrite,
            Self::Rename { .. } => super::Command::Rename,
            Self::Move { .. } => super::Command::Move,
            Self::MoveSymbol { .. } => super::Command::MoveSymbol,
            Self::Batch { .. } => super::Command::Batch,
            Self::History => super::Command::History,
            Self::Undo => super::Command::Undo,
        }
    }
}

/// The answer to a [`Request`]: the result type of the command asked. On
/// the wire it is that type's shape alone — a client knows what it asked —
/// so it serialises without a tag and is read back by the command's type.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
// Built once and serialised once: the largest result's size is not a cost
// worth a `Box` in every client's match.
#[allow(clippy::large_enum_variant)]
pub enum Answer {
    DiscardPlan(crate::PlanReview),
    ApplyPlan(crate::PlanReceipt),
    ValidatePlan(crate::ValidationReport),
    InspectPlan(crate::PlanReviewReply),
    PrepareRename(crate::PlanReviewReply),
    PrepareRewrite(crate::PlanReviewReply),
    ReviewPlan(crate::PlanReviewPage),
    #[cfg(feature = "schema")]
    Schema(crate::SchemaDocument),
    SearchPage(crate::SearchPage),
    ContextPage(crate::ContextPage),
    Continue(crate::PageReply),
    Expand(crate::Expansion),
    Discover(crate::Discovery),
    Context(crate::ContextReply),
    Navigate(crate::NavigationReply),
    Relationships(crate::Relationships),
    Resolve(crate::ResolutionReply),
    Search(Search),
    Outline(Outline),
    References(References),
    Where(Locations),
    Deps(Deps),
    Explain(Explanation),
    Surface(Surface),
    Impact(Impact),
    Dead(Dead),
    Imports(ImportsReport),
    File(File),
    Rewrite(Rewrite),
    Rename(Rename),
    Move(Move),
    MoveSymbol(MoveSymbol),
    Batch(Batch),
    History(History),
    Undo(Undo),
}

impl Answer {
    /// The notices a mutating answer leaves to a person, if any.
    pub fn notices(&self) -> &[Notice] {
        match self {
            Self::Move(m) => &m.notices,
            Self::MoveSymbol(m) => &m.notices,
            Self::Batch(b) => &b.notices,
            _ => &[],
        }
    }

    /// The history entry a written answer recorded, if any.
    pub fn history_id(&self) -> Option<u64> {
        match self {
            Self::ApplyPlan(r) => Some(r.history_id),
            Self::Rewrite(r) => r.state.history_id(),
            Self::Rename(r) => r.state.history_id(),
            Self::Move(r) => r.state.history_id(),
            Self::MoveSymbol(r) => r.state.history_id(),
            Self::Batch(b) => b.state.history_id(),
            _ => None,
        }
    }
}

impl From<MutationAnswer> for Answer {
    fn from(result: MutationAnswer) -> Self {
        match result {
            MutationAnswer::Rewrite(result) => Self::Rewrite(result),
            MutationAnswer::Rename(result) => Self::Rename(result),
            MutationAnswer::Move(result) => Self::Move(result),
            MutationAnswer::MoveSymbol(result) => Self::MoveSymbol(result),
            MutationAnswer::Batch(result) => Self::Batch(result),
        }
    }
}

/// A request on a session's wire (`vvv serve`): the request with whatever
/// `id` the caller chose, echoed on the [`Reply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Call {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(range(min = crate::ContextBudget::MIN_BYTES, max = crate::ContextBudget::MAX_BYTES)))]
    pub max_output_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(flatten)]
    pub request: Request,
}

/// The reply to a [`Call`]: its `id`, then the usual envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Reply<T> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(flatten)]
    pub response: Response<T>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_read_only_guard_separates_queries_from_mutations() {
        assert!(Request::History.is_read_only());
        assert!(Request::Search(Query::pattern("Engine").into()).is_read_only());
        assert!(Request::References(ReferencesQuery::new("Engine")).is_read_only());
        assert!(!Request::Undo.is_read_only(), "undo writes, however small");
        assert!(
            !Request::Rename {
                intent: RenameIntent::new("A", "B"),
                apply: false,
            }
            .is_read_only()
        );
        assert!(
            !Request::Batch {
                intent: BatchIntent { intents: vec![] },
                apply: false,
            }
            .is_read_only()
        );
    }
}
