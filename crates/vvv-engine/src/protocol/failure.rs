//! What went wrong, as data: a code a client can branch on, the message a
//! person reads, and the hint that says what to try instead.

use crate::EngineError;
use serde::{Deserialize, Serialize};

/// The kind of failure, stable across releases; the message is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidPlan,
    PlanExpired,
    PlanConsumed,
    CursorExpired,
    InvalidCursor,
    RetentionLimit,
    OutputLimit,
    /// The caller cancelled the operation.
    Cancelled,
    /// Resolution stopped before a complete answer was available.
    Incomplete,
    /// The request could not be read: not JSON, or not a known command.
    BadRequest,
    /// A query with none of pattern, kind, symbol, name.
    BadQuery,
    /// A pattern or node kind the language's grammar rejects.
    BadPattern,
    /// A selection naming rows or ids the search did not find.
    BadSelection,
    /// A template naming a capture the match does not have.
    BadTemplate,
    /// No declaration by that name (and kind).
    NoSuchSymbol,
    /// Several declarations share the name; `declared_in` picks one.
    AmbiguousSymbol,
    /// No registered language claims the file.
    NoLanguage,
    /// The language has no layout: paths cannot be followed, files not moved.
    NoLayout,
    /// A position past the end of the file.
    NoSuchPosition,
    /// The destination already exists.
    Exists,
    /// A file, or the file declaring a name, could not be found.
    NotFound,
    /// The layout refuses the move: a root, across packages, into itself.
    Unmovable,
    /// Two edits of one plan overlap, or a file is moved twice.
    Conflict,
    /// A file changed since the plan (or the apply to undo) was made.
    Stale,
    /// Nothing to undo, or a history file that cannot be read.
    NoHistory,
    /// Reading or writing the tree failed.
    Io,
    /// Recovery could not restore or verify every attempted effect.
    RecoveryFailed,
}

/// A failed request, as the wire carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Failure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<ContinuationRecovery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_limit: Option<OutputLimit>,
    pub code: ErrorCode,
    pub message: String,
    /// What to try instead, when there is something.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Effects that could not be restored after a failed mutation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Recovery>,
}

impl Failure {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            recovery: None,
            output_limit: None,
            continuation: None,
        }
    }

    pub fn with_recovery(mut self, recovery: Recovery) -> Self {
        self.recovery = Some(recovery);
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl From<&EngineError> for Failure {
    fn from(error: &EngineError) -> Self {
        let mut failure = Failure::new(error.code(), format!("{error:#}"));
        if let EngineError::Recovery(recovery) = error {
            failure = failure.with_recovery(recovery.details.clone());
        }
        if let EngineError::OutputLimit {
            max_bytes,
            required_bytes,
        } = error
        {
            failure.output_limit = Some(OutputLimit {
                max_bytes: *max_bytes,
                required_bytes: *required_bytes,
                anchor: None,
            });
        }
        failure.continuation = match error {
            EngineError::StaleQuery | EngineError::CursorExpired => {
                Some(ContinuationRecovery::RestartQuery)
            }
            EngineError::InvalidCursor => Some(ContinuationRecovery::CorrectCursor),
            EngineError::RetentionLimit => Some(ContinuationRecovery::NarrowQuery),
            EngineError::PageOutputLimit {
                max_bytes,
                required_bytes,
                anchor,
            } => {
                failure.output_limit = Some(OutputLimit {
                    max_bytes: *max_bytes,
                    required_bytes: *required_bytes,
                    anchor: anchor.clone().map(Box::new),
                });
                Some(ContinuationRecovery::IncreaseBudgetOrNarrowQuery)
            }
            EngineError::OutputLimit { .. } => {
                Some(ContinuationRecovery::IncreaseBudgetOrNarrowQuery)
            }
            _ => None,
        };
        match error.hint() {
            Some(hint) => failure.with_hint(hint),
            None => failure,
        }
    }
}

/// A recovery result; unknown states are separate from confirmed residual effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Recovery {
    pub cause: Box<Failure>,
    pub failures: Vec<RecoveryIssue>,
    pub remaining: Vec<RecoveryEffect>,
    pub unverified: Vec<RecoveryUnverified>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RecoveryIssue {
    pub operation: RecoveryOperation,
    pub path: crate::RelPath,
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RecoveryUnverified {
    pub path: crate::RelPath,
    pub expected: RecoveryState,
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecoveryOperation {
    RestoreFile,
    RestoreMove,
    RestoreDirectory,
    RemoveFile,
    RemoveDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RecoveryEffect {
    pub path: crate::RelPath,
    pub expected: RecoveryState,
    pub observed: RecoveryState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecoveryState {
    Absent,
    File {
        fingerprint: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spelling: Option<crate::RelPath>,
    },
    Directory,
    Other,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorCode;

    /// A client branches on `code`, so its spelling is part of the protocol
    /// and renaming one is a breaking change. The exhaustive `match` makes a
    /// new variant a compile error here until its spelling joins the table.
    #[test]
    fn every_code_keeps_its_wire_spelling() {
        for code in [
            ErrorCode::InvalidPlan,
            ErrorCode::PlanExpired,
            ErrorCode::PlanConsumed,
            ErrorCode::CursorExpired,
            ErrorCode::InvalidCursor,
            ErrorCode::RetentionLimit,
            ErrorCode::OutputLimit,
            ErrorCode::Cancelled,
            ErrorCode::Incomplete,
            ErrorCode::BadRequest,
            ErrorCode::BadQuery,
            ErrorCode::BadPattern,
            ErrorCode::BadSelection,
            ErrorCode::BadTemplate,
            ErrorCode::NoSuchSymbol,
            ErrorCode::AmbiguousSymbol,
            ErrorCode::NoLanguage,
            ErrorCode::NoLayout,
            ErrorCode::NoSuchPosition,
            ErrorCode::Exists,
            ErrorCode::NotFound,
            ErrorCode::Unmovable,
            ErrorCode::Conflict,
            ErrorCode::Stale,
            ErrorCode::NoHistory,
            ErrorCode::Io,
            ErrorCode::RecoveryFailed,
        ] {
            let documented = match code {
                ErrorCode::InvalidPlan => "invalid_plan",
                ErrorCode::PlanExpired => "plan_expired",
                ErrorCode::PlanConsumed => "plan_consumed",
                ErrorCode::CursorExpired => "cursor_expired",
                ErrorCode::InvalidCursor => "invalid_cursor",
                ErrorCode::RetentionLimit => "retention_limit",
                ErrorCode::OutputLimit => "output_limit",
                ErrorCode::Cancelled => "cancelled",
                ErrorCode::Incomplete => "incomplete",
                ErrorCode::BadRequest => "bad_request",
                ErrorCode::BadQuery => "bad_query",
                ErrorCode::BadPattern => "bad_pattern",
                ErrorCode::BadSelection => "bad_selection",
                ErrorCode::BadTemplate => "bad_template",
                ErrorCode::NoSuchSymbol => "no_such_symbol",
                ErrorCode::AmbiguousSymbol => "ambiguous_symbol",
                ErrorCode::NoLanguage => "no_language",
                ErrorCode::NoLayout => "no_layout",
                ErrorCode::NoSuchPosition => "no_such_position",
                ErrorCode::Exists => "exists",
                ErrorCode::NotFound => "not_found",
                ErrorCode::Unmovable => "unmovable",
                ErrorCode::Conflict => "conflict",
                ErrorCode::Stale => "stale",
                ErrorCode::NoHistory => "no_history",
                ErrorCode::Io => "io",
                ErrorCode::RecoveryFailed => "recovery_failed",
            };
            assert_eq!(
                serde_json::to_value(code).unwrap(),
                documented,
                "docs/protocol.md documents `{documented}`"
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OutputLimit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Box<crate::SourceAnchor>>,
    pub max_bytes: usize,
    pub required_bytes: usize,
}

/// A recovery action clients can branch on without parsing the error message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ContinuationRecovery {
    RestartQuery,
    CorrectCursor,
    IncreaseBudgetOrNarrowQuery,
    NarrowQuery,
}
