//! What each command answers with. A mutating command answers the same shape
//! whether it previewed or wrote: `applied` and `history_id` say which.

use vvv_core::RelPath;

use serde::{Deserialize, Serialize};
use vvv_core::Edit;

use super::diff::Diff;
use super::{Intent, Notice};
use crate::batch::Batch;
use crate::capabilities::moves::{Move, MoveSymbol};
use crate::capabilities::rename::Rename;
use crate::rewrite::Rewrite;

/// The lifecycle of a mutation result, with a history id exactly when applied.
/// Serializes as `applied` plus the optional `history_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MutationStateFields", into = "MutationStateFields")]
pub enum MutationState {
    Preview,
    Applied { history_id: u64 },
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for MutationState {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "MutationState".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let preview_history = if generator.contract().is_deserialize() {
            serde_json::json!({"type": "null"})
        } else {
            serde_json::json!(false)
        };
        schemars::json_schema!({
            "type": "object",
            "oneOf": [
                {
                    "properties": {
                        "applied": {"const": false},
                        "history_id": preview_history
                    },
                    "required": ["applied"]
                },
                {
                    "properties": {
                        "applied": {"const": true},
                        "history_id": generator.subschema_for::<u64>()
                    },
                    "required": ["applied", "history_id"]
                }
            ]
        })
    }
}

impl MutationState {
    pub fn is_applied(self) -> bool {
        matches!(self, Self::Applied { .. })
    }

    pub fn history_id(self) -> Option<u64> {
        match self {
            Self::Preview => None,
            Self::Applied { history_id } => Some(history_id),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct MutationStateFields {
    applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    history_id: Option<u64>,
}

impl From<MutationState> for MutationStateFields {
    fn from(state: MutationState) -> Self {
        Self {
            applied: state.is_applied(),
            history_id: state.history_id(),
        }
    }
}

impl TryFrom<MutationStateFields> for MutationState {
    type Error = MutationStateError;

    fn try_from(fields: MutationStateFields) -> Result<Self, Self::Error> {
        match (fields.applied, fields.history_id) {
            (false, None) => Ok(Self::Preview),
            (true, Some(history_id)) => Ok(Self::Applied { history_id }),
            (false, Some(_)) => Err(MutationStateError::PreviewWithHistory),
            (true, None) => Err(MutationStateError::AppliedWithoutHistory),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum MutationStateError {
    #[error("a preview cannot have a history_id")]
    PreviewWithHistory,
    #[error("an applied mutation requires a history_id")]
    AppliedWithoutHistory,
}

/// One apply that can still be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HistoryEntry {
    pub id: u64,
    /// Seconds since the Unix epoch.
    pub at: u64,
    /// What was applied, as data; the display layer renders it.
    pub intent: Intent,
    /// How many files it wrote.
    pub files: usize,
    /// The files it wrote, at their pre-apply paths: what undo restores.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<RelPath>,
    /// Moves it made, as (from, to) pairs: what undo reverts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moves: Vec<(RelPath, RelPath)>,
}

/// `vvv history`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct History {
    /// Oldest first; the last entry is what `vvv undo` would reverse.
    pub entries: Vec<HistoryEntry>,
}

/// `vvv undo`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Undo {
    pub undone: HistoryEntry,
    /// Files restored to their pre-apply contents, at their pre-apply paths.
    pub restored: Vec<RelPath>,
    /// Moves reverted, as (original, moved-to) pairs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moves_reverted: Vec<(RelPath, RelPath)>,
}

/// One file a plan touches: its edits and the whole-file diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileChange {
    /// Path before the change.
    pub path: RelPath,
    /// Path after, when the file is moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_to: Option<RelPath>,
    pub edits: Vec<Edit>,
    pub diff: Diff,
}

/// The closed set of mutation payloads an executable plan can carry.
/// External payloads cannot authorize writes:
///
/// ```compile_fail,E0277
/// use vvv_engine::{Mutation, MutationAnswer};
/// struct External;
/// impl Mutation for External {
///     fn into_mutation(self) -> MutationAnswer { unimplemented!() }
///     fn applied(&mut self, _: u64) {}
/// }
/// ```
pub trait Mutation: sealed::Sealed {
    /// Wrap this payload in the closed mutation result.
    fn into_mutation(self) -> MutationAnswer;
    /// Mark the result as written by history entry `id`.
    fn applied(&mut self, id: u64);
}

/// The result of a mutation intent. Queries have no variant here.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum MutationAnswer {
    Rewrite(Rewrite),
    Rename(Rename),
    Move(Move),
    MoveSymbol(MoveSymbol),
    Batch(Batch),
}

impl MutationAnswer {
    /// Notices retained by a mutation, if any.
    pub fn notices(&self) -> &[Notice] {
        match self {
            Self::Rewrite(_) | Self::Rename(_) => &[],
            Self::Move(result) => &result.notices,
            Self::MoveSymbol(result) => &result.notices,
            Self::Batch(result) => &result.notices,
        }
    }

    /// The history entry recorded by an applied mutation.
    pub fn history_id(&self) -> Option<u64> {
        match self {
            Self::Rewrite(result) => result.state.history_id(),
            Self::Rename(result) => result.state.history_id(),
            Self::Move(result) => result.state.history_id(),
            Self::MoveSymbol(result) => result.state.history_id(),
            Self::Batch(result) => result.state.history_id(),
        }
    }
}

impl Mutation for MutationAnswer {
    fn into_mutation(self) -> Self {
        self
    }

    fn applied(&mut self, id: u64) {
        match self {
            Self::Rewrite(result) => result.applied(id),
            Self::Rename(result) => result.applied(id),
            Self::Move(result) => result.applied(id),
            Self::MoveSymbol(result) => result.applied(id),
            Self::Batch(result) => result.applied(id),
        }
    }
}

impl TryFrom<super::Answer> for MutationAnswer {
    type Error = super::Answer;

    fn try_from(answer: super::Answer) -> Result<Self, Self::Error> {
        match answer {
            super::Answer::Rewrite(result) => Ok(Self::Rewrite(result)),
            super::Answer::Rename(result) => Ok(Self::Rename(result)),
            super::Answer::Move(result) => Ok(Self::Move(result)),
            super::Answer::MoveSymbol(result) => Ok(Self::MoveSymbol(result)),
            super::Answer::Batch(result) => Ok(Self::Batch(result)),
            query => Err(query),
        }
    }
}

// Keep executable payload membership beside the closed MutationAnswer sum.
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Rewrite {}
    impl Sealed for super::Rename {}
    impl Sealed for super::Move {}
    impl Sealed for super::MoveSymbol {}
    impl Sealed for super::Batch {}
    impl Sealed for super::MutationAnswer {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mutation_states_keep_the_existing_wire_fields() {
        for (state, wire) in [
            (MutationState::Preview, json!({"applied": false})),
            (
                MutationState::Applied { history_id: 7 },
                json!({"applied": true, "history_id": 7}),
            ),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<MutationState>(wire).unwrap(),
                state
            );
        }
    }

    #[test]
    fn previews_accept_an_omitted_or_null_history_id() {
        for wire in [
            json!({"applied": false}),
            json!({"applied": false, "history_id": null}),
        ] {
            assert_eq!(
                serde_json::from_value::<MutationState>(wire).unwrap(),
                MutationState::Preview
            );
        }
    }

    #[test]
    fn contradictory_mutation_states_are_rejected() {
        for (wire, message) in [
            (
                json!({"applied": false, "history_id": 7}),
                "a preview cannot have a history_id",
            ),
            (
                json!({"applied": true}),
                "an applied mutation requires a history_id",
            ),
            (
                json!({"applied": true, "history_id": null}),
                "an applied mutation requires a history_id",
            ),
        ] {
            let error = serde_json::from_value::<MutationState>(wire).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
        }
    }
}
