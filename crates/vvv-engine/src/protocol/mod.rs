//! The JSON contract. Anything printed by `vvv --json` is one of these types,
//! so a client can rely on the shape independently of the CLI's human output.
//!
//! Data only: nothing here reads a file or takes a `Workspace`. A client that
//! only speaks JSON depends on this crate alone.

mod answer;
mod command;
pub use command::Command;
mod diff;
pub mod display;
mod failure;
mod intent;
mod notice;
mod reach;
mod request;
mod respelling;
mod result;
mod search;
mod selection;
mod source;
mod template;
pub mod vocabulary;

use serde::{Deserialize, Serialize};

pub use crate::batch::Batch;
pub use crate::capabilities::declarations::{
    Locations, Outline, OutlineItem, OutlineQuery, Site, WhereQuery,
};
pub use crate::capabilities::file::{File, FileQuery};
pub use crate::capabilities::imports::{
    Deps, DepsQuery, ExplainQuery, Explanation, ImportSite, ImportsQuery, ImportsReport,
};
pub use crate::capabilities::rename::{Definitions, References, ReferencesQuery};
pub use crate::capabilities::search::{Search, SearchQuery, SearchScope};
pub use crate::capabilities::surface::{Exposed, Surface, SurfaceQuery};
pub use crate::capabilities::usage::{
    Consumer, Dead, DeadQuery, Impact, ImpactQuery, Unreferenced,
};
pub use crate::rewrite::Rewrite;
pub use answer::{Dep, Importer, Placed};
pub use diff::{Diff, DiffKind, DiffLine, Hunk, LineRange};
pub use failure::{
    ErrorCode, Failure, OutputLimit, Recovery, RecoveryEffect, RecoveryIssue, RecoveryOperation,
    RecoveryState, RecoveryUnverified,
};
pub use intent::{BatchIntent, Intent, RewriteIntent, RewriteOf};
pub use notice::{Notice, NoticeKind};

pub use reach::Reach;
pub use request::{Answer, Call, Reply, Request};
pub use respelling::Respelling;
pub use result::{
    FileChange, History, HistoryEntry, Mutation, MutationAnswer, MutationState, Undo,
};
pub use search::{Confidence, Match, MatchId, Occurrence, Reason, Skipped};
pub use selection::{Selection, SelectionError};
pub use template::{Template, TemplateError};

/// The shape of this contract. Bumped when a field changes meaning or goes
/// away; adding fields does not bump it.
pub const SCHEMA: u32 = 1;

/// Top-level envelope of every `--json` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Response<T> {
    Ok {
        schema: u32,
        result: T,
    },
    Error {
        schema: u32,
        #[serde(flatten)]
        failure: Failure,
    },
}

impl<T> Response<T> {
    pub fn ok(result: T) -> Self {
        Self::Ok {
            schema: SCHEMA,
            result,
        }
    }

    pub fn error(failure: Failure) -> Self {
        Self::Error {
            schema: SCHEMA,
            failure,
        }
    }
}

pub use crate::capabilities::navigation::{
    DefinitionCandidate, DefinitionLocation, DefinitionPreview, NavigationOrigin,
    NavigationOutcome, NavigationQuery, NavigationReply, ResolutionEvidence, ResolutionOutcome,
    ResolutionQuery, ResolutionReply, SnapshotId, UnavailableReason,
};
pub use source::{ContentId, SourceAnchor, SymbolRef};

pub use crate::capabilities::context::{
    ContextBudget, ContextCandidate, ContextDetail, ContextItem, ContextOmissions, ContextOutcome,
    ContextQuery, ContextRelation, ContextReply, ContextSignature,
};

pub use crate::capabilities::discovery::{Capability, Discovery, DiscoveryQuery};

#[cfg(feature = "schema")]
pub use crate::capabilities::schema::{
    SchemaContract, SchemaDocument, SchemaQuery, SchemaReferences,
};

mod cursor;
pub use crate::capabilities::context::{
    ContextPage, ContextPageQuery, ContextUnresolved, ContextWork, PagedContextItem,
};
pub use crate::capabilities::excerpts::{ExpandQuery, Expansion};
pub use crate::capabilities::pagination::{ContinueQuery, PageBudget, PageReply, WorkBudget};
pub use crate::capabilities::search::{SearchPage, SearchPageItem, SearchPageQuery};
pub use crate::query_store::QueryLimits;
pub use cursor::Cursor;

pub use failure::ContinuationRecovery;

pub use crate::capabilities::relationships::{
    Relationship, RelationshipBudget, RelationshipCoverage, RelationshipKind, RelationshipLimit,
    RelationshipLimitation, RelationshipResolution, Relationships, RelationshipsQuery,
};

pub use crate::capabilities::plans::{
    ApplyPlanQuery, DiscardPlanQuery, InspectPlanQuery, PlanId, PlanPreview, PlanReceipt,
    PlanReview, PlanReviewCursor, PlanReviewItem, PlanReviewKind, PlanReviewPage, PlanReviewReply,
    PlanStatus, PrepareMoveIntent, PrepareMoveQuery, PrepareRenameQuery, PrepareRewriteQuery,
    ReviewMutation, ReviewPlanQuery, ReviewSection, ReviewTotals,
};

pub use crate::capabilities::validation::{
    CheckCommand, CheckFailure, CheckOperation, CheckOutcome, CheckOutput, CheckResult,
    ValidatePlanQuery, ValidationBudget, ValidationReport, ValidationSourceState,
};
