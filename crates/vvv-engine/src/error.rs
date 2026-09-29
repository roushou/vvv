use crate::{ApplyError, ErrorCode, SelectionError, TemplateError, VfsError};

use vvv_core::RelPath;
use vvv_core::{
    EditConflict, LanguageId, Position, QueryError, ResolveError, SearchError, SymbolKind,
};

use crate::history::HistoryError;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("select exactly one symbol-move declaration: {}", candidates.iter().enumerate().map(|(index, candidate)| format!("{}: {} {}:{}", index + 1, candidate.declaration.id, candidate.declaration.path, candidate.declaration.start.line + 1)).collect::<Vec<_>>().join(", "))]
    SymbolMoveSelection {
        candidates: Vec<crate::SymbolMoveCandidate>,
    },
    #[error("selected declaration cannot be moved: {reason}")]
    UnsupportedSymbolMove {
        declaration: Box<crate::Match>,
        reason: crate::SymbolMoveUnsupported,
    },
    #[error("invalid symbol-move ownership evidence")]
    InvalidSymbolMoveEvidence,
    #[error("validation requires a disk workspace on a supported platform")]
    ValidationUnavailable,
    #[error(
        "validation requires an applied plan, one to four valid check commands, and relative extra input paths"
    )]
    InvalidValidation,
    #[error("invalid plan handle")]
    InvalidPlan,
    #[error("move paths must be nonempty workspace-relative paths without .. components: {path}")]
    InvalidMovePath { path: RelPath },
    #[error("plan expired or belongs to another session; prepare and review a new plan")]
    PlanExpired,
    #[error("plan cannot be applied again; inspect its terminal outcome")]
    PlanConsumed,
    #[error("workspace changed since review; prepare and review a new plan")]
    StalePlan,
    #[error("plan retention limit reached; discard pending plans or wait for expiry")]
    PlanRetentionLimit,
    #[error(
        "search paths must be workspace-relative with / separators and no .. components; package filters must be nonempty"
    )]
    InvalidSearchScope,
    #[error("request cancelled")]
    ReadCancelled,
    #[error("a cancellation handle can execute only one call")]
    ReusedCancellation,
    #[error("cooperative cancellation is only supported for reads and explicit validation")]
    MutationCancellation,
    #[error("query sources changed; start a new query")]
    StaleQuery,
    #[error("cursor expired or belongs to another session; start a new query")]
    CursorExpired,
    #[error("invalid cursor or wrong cursor kind")]
    InvalidCursor,
    #[error("query exceeds retained-state limits; narrow the request")]
    RetentionLimit,
    #[error("result needs {required_bytes} bytes; output budget is {max_bytes}")]
    PageOutputLimit {
        max_bytes: usize,
        required_bytes: usize,
        anchor: Option<crate::SourceAnchor>,
    },
    #[cfg(feature = "schema")]
    #[error("command schemas require for_command; call and reply schemas do not accept it")]
    InvalidSchemaQuery,
    #[error("invalid output or context budget")]
    InvalidBudget,
    #[error("output budgets are only supported for read-only requests")]
    MutationBudget,
    #[error("result needs {required_bytes} bytes; output budget is {max_bytes}")]
    OutputLimit {
        max_bytes: usize,
        required_bytes: usize,
    },
    #[error("navigation cancelled")]
    NavigationCancelled,
    #[error("semantic provider state changed")]
    StaleSemantic,
    #[error("semantic provider returned invalid or inconsistent source evidence")]
    InvalidSemantic,

    #[error("source changed: {path}")]
    StaleSource { path: RelPath },
    #[error("invalid navigation range in {path}")]
    InvalidAnchor { path: RelPath },
    #[error("navigation exceeded its resolution budget")]
    NavigationLimit,
    #[error("select exactly one navigation candidate")]
    NavigationSelection,

    #[error("expected {expected} execution, got {actual}")]
    ExecutionKind {
        expected: crate::ExecutionKind,
        actual: crate::ExecutionKind,
    },
    #[error("{}: {source}", path.display())]
    Open {
        path: RelPath,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Vfs(#[from] VfsError),
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error("search failed in {}", path.display())]
    Search {
        path: RelPath,
        #[source]
        source: SearchError,
    },
    #[error(transparent)]
    Selection(#[from] SelectionError),
    #[error("{}:{}", path.display(), line + 1)]
    Template {
        path: RelPath,
        line: u32,
        #[source]
        source: TemplateError,
    },
    #[error(transparent)]
    Conflict(#[from] EditConflict),
    #[error(transparent)]
    Extraction(#[from] crate::ExtractionError),
    #[error(transparent)]
    Regroup(#[from] vvv_core::RegroupError),
    #[error("`{name}` is declared in several places ({})", declarations.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))]
    AmbiguousSymbol {
        name: String,
        declarations: Vec<crate::graph::DeclarationSite>,
    },
    #[error("no {} named `{name}`", kind.map_or("symbol", SymbolKind::as_str))]
    NoSuchSymbol {
        name: String,
        kind: Option<SymbolKind>,
    },
    #[error("{} already exists", .0.display())]
    Exists(RelPath),
    #[error("no registered language claims {}", .0.display())]
    NoLanguage(RelPath),
    #[error("{0} has no layout yet, so its paths cannot be followed or its files moved")]
    NoLayout(LanguageId),
    #[error("{}:{} is past the end of the file", path.display(), position.display())]
    NoSuchPosition { path: RelPath, position: Position },
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Apply(#[from] ApplyError),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error(transparent)]
    Recovery(#[from] RecoveryError),
}

impl EngineError {
    /// The stable code a client branches on.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::SymbolMoveSelection { .. } => ErrorCode::BadSelection,
            Self::UnsupportedSymbolMove { .. } => ErrorCode::Unmovable,
            Self::InvalidSymbolMoveEvidence => ErrorCode::Conflict,
            Self::InvalidPlan => ErrorCode::InvalidPlan,
            Self::PlanExpired => ErrorCode::PlanExpired,
            Self::PlanConsumed => ErrorCode::PlanConsumed,
            Self::StalePlan => ErrorCode::Stale,
            Self::PlanRetentionLimit => ErrorCode::RetentionLimit,
            Self::InvalidSearchScope | Self::InvalidMovePath { .. } => ErrorCode::BadRequest,
            Self::ReadCancelled => ErrorCode::Cancelled,
            Self::ReusedCancellation | Self::MutationCancellation => ErrorCode::BadRequest,
            #[cfg(feature = "schema")]
            Self::InvalidSchemaQuery => ErrorCode::BadRequest,
            Self::InvalidBudget
            | Self::InvalidValidation
            | Self::ValidationUnavailable
            | Self::MutationBudget => ErrorCode::BadRequest,
            Self::StaleQuery => ErrorCode::Stale,
            Self::CursorExpired => ErrorCode::CursorExpired,
            Self::InvalidCursor => ErrorCode::InvalidCursor,
            Self::RetentionLimit => ErrorCode::RetentionLimit,
            Self::PageOutputLimit { .. } | Self::OutputLimit { .. } => ErrorCode::OutputLimit,
            Self::NavigationCancelled => ErrorCode::Cancelled,
            Self::StaleSemantic => ErrorCode::Stale,
            Self::InvalidSemantic => ErrorCode::BadRequest,
            Self::StaleSource { .. } => ErrorCode::Stale,
            Self::InvalidAnchor { .. } => ErrorCode::BadRequest,
            Self::NavigationSelection => ErrorCode::BadSelection,
            Self::NavigationLimit => ErrorCode::Incomplete,
            Self::ExecutionKind { .. } => ErrorCode::BadRequest,
            Self::Open { .. } => ErrorCode::Io,
            Self::Vfs(e)
            | Self::History(HistoryError::Vfs(e))
            | Self::Apply(ApplyError::Vfs(e)) => match e {
                VfsError::NotFound(_) => ErrorCode::NotFound,
                VfsError::Exists(_) => ErrorCode::Exists,
                _ => ErrorCode::Io,
            },
            Self::Query(_) => ErrorCode::BadQuery,
            Self::Search { .. } => ErrorCode::BadPattern,
            Self::Selection(_) => ErrorCode::BadSelection,
            Self::Template { .. } => ErrorCode::BadTemplate,
            Self::Conflict(_) | Self::Extraction(_) | Self::Regroup(_) => ErrorCode::Conflict,
            Self::AmbiguousSymbol { .. } => ErrorCode::AmbiguousSymbol,
            Self::NoSuchSymbol { .. } => ErrorCode::NoSuchSymbol,
            Self::Exists(_) => ErrorCode::Exists,
            Self::NoLanguage(_) => ErrorCode::NoLanguage,
            Self::NoLayout(_) => ErrorCode::NoLayout,
            Self::NoSuchPosition { .. } => ErrorCode::NoSuchPosition,
            Self::Resolve(e) => match e {
                ResolveError::Missing(_) | ResolveError::NoDeclaringFile { .. } => {
                    ErrorCode::NotFound
                }
                _ => ErrorCode::Unmovable,
            },
            Self::Apply(e) => match e {
                ApplyError::DestinationExists { .. } => ErrorCode::Exists,
                ApplyError::Stale { .. } | ApplyError::Modified { .. } => ErrorCode::Stale,
                _ => ErrorCode::Io,
            },
            Self::History(_) => ErrorCode::NoHistory,
            Self::Recovery(_) => ErrorCode::RecoveryFailed,
        }
    }

    /// What to try instead, when the error suggests something.
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::InvalidPlan => Some("Pass the exact plan_id returned by preparation".into()),
            Self::PlanExpired | Self::StalePlan => Some("Prepare a new plan and review its diff before applying".into()),
            Self::PlanConsumed => Some("Use inspect_plan to retrieve the recorded outcome; do not retry by rebuilding the mutation".into()),
            Self::PlanRetentionLimit => Some("Discard pending plans, narrow the rename, or wait for terminal outcomes to expire".into()),
            Self::InvalidBudget => Some("Use discover to check supported output and context budget ranges".into()),
            Self::OutputLimit { .. } => Some("Increase max_output_bytes, narrow the query, or request bounded context".into()),
            Self::NoSuchSymbol { name, .. } => Some(format!(
                "declarations are matched by exact name; `vvv search --name {name}` shows what exists"
            )),
            Self::AmbiguousSymbol { declarations, .. } => Some(format!(
                "for example: --in {}",
                declarations
                    .first()
                    .map(|d| d.path.display().to_string())
                    .unwrap_or_default()
            )),
            Self::Query(_) => Some(
                "search by pattern:  vvv search 'fn $NAME($$$) { $$$ }'\nby declaration:     vvv search --symbol trait   |   vvv search --name Foo"
                    .to_owned(),
            ),
            _ => None,
        }
    }
}

/// A failed mutation whose effects could not all be restored and verified.
#[derive(Debug)]
pub struct RecoveryError {
    pub cause: Box<EngineError>,
    pub details: crate::Recovery,
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}; recovery incomplete", self.cause)?;
        for effect in &self.details.remaining {
            write!(
                f,
                "; {}: expected {:?}, observed {:?}",
                effect.path.display(),
                effect.expected,
                effect.observed
            )?;
        }
        for issue in &self.details.unverified {
            write!(
                f,
                "; {}: state unverified ({})",
                issue.path.display(),
                issue.message
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for RecoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}
