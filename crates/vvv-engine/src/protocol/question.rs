//! The read-only requests, as data: what an interface asks when it wants to
//! understand rather than change. Each is a command whose answer is the
//! result type of the same name.

use vvv_core::RelPath;

use serde::{Deserialize, Serialize};
use vvv_core::Position;

/// `vvv outline <path>`: what a file declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineQuery {
    pub path: RelPath,
}

/// `vvv deps <path>`: what a file imports and who imports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepsQuery {
    pub path: RelPath,
}

/// `vvv explain <path>:<line>:<col>`: what is at a position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainQuery {
    pub path: RelPath,
    pub position: Position,
}

/// `vvv where <name> [--from <file>]`: where a name is declared, and the
/// import that reaches each site from `from`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhereQuery {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<RelPath>,
}

/// One file as it is now, with its syntax colouring: what a picker shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileQuery {
    pub path: RelPath,
}

/// The applies that can still be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HistoryQuery;

/// Reverse the most recent apply, provided its files are untouched since.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UndoLast;

/// `vvv surface [package]`: what a package offers to everyone.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SurfaceQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

/// `vvv impact <name>`: who would feel a change to a declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactQuery {
    pub name: String,
    /// The file declaring the symbol meant, when several share the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_in: Option<RelPath>,
}

/// `vvv dead`: declarations nothing in the workspace refers to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DeadQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<vvv_core::LanguageId>,
}

/// `vvv imports [path]`: import statements worth a look.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ImportsQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<RelPath>,
}
