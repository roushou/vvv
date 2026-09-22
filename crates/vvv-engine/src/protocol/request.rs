//! Every request as one value, and every answer as one: what a client sends
//! down a wire and reads back. `{"command": "rename", "name": "Config",
//! "to": "Settings", "apply": true}` is a [`Request`]; the reply's `result`
//! is the [`Answer`] of the same name.

use serde::{Deserialize, Serialize};
use vvv_core::Query;

use super::{
    Batch, BatchIntent, Dead, DeadQuery, Deps, DepsQuery, ExplainQuery, Explanation, File,
    FileQuery, History, Impact, ImpactQuery, ImportsQuery, ImportsReport, Locations, Move,
    MoveIntent, MoveSymbol, MoveSymbolIntent, Notice, Outline, OutlineQuery, References,
    ReferencesQuery, Rename, RenameIntent, Response, Rewrite, RewriteIntent, Search, Surface,
    SurfaceQuery, Undo, WhereQuery,
};

/// One request, tagged by `command`. A mutation carries the same fields as
/// its [`Intent`](super::Intent) plus `apply`: false previews, true writes
/// and records one undo — exactly what the CLI's `--apply` does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    Search(Query),
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

/// The answer to a [`Request`]: the result type of the command asked. On
/// the wire it is that type's shape alone — a client knows what it asked —
/// so it serialises without a tag and is read back by the command's type.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
// Built once and serialised once: the largest result's size is not a cost
// worth a `Box` in every client's match.
#[allow(clippy::large_enum_variant)]
pub enum Answer {
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
            Self::Rewrite(r) => r.history_id,
            Self::Rename(r) => r.history_id,
            Self::Move(r) => r.history_id,
            Self::MoveSymbol(r) => r.history_id,
            Self::Batch(b) => b.history_id,
            _ => None,
        }
    }
}

/// A request on a session's wire (`vvv serve`): the request with whatever
/// `id` the caller chose, echoed on the [`Reply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(flatten)]
    pub request: Request,
}

/// The reply to a [`Call`]: its `id`, then the usual envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply<T> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(flatten)]
    pub response: Response<T>,
}
