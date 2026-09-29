//! What a question about the tree comes back as: plain data over the facts
//! the model holds, for the CLI to print, the JSON to carry, an agent to read.

use vvv_core::RelPath;

use serde::{Deserialize, Serialize};

use crate::Reach;
use vvv_core::{Address, ImportRef, Position, Symbol};

/// One import statement in the file and, when the layout can follow it,
/// where it leads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Dep {
    #[serde(flatten)]
    pub import: ImportRef,
    /// Where the import starts, as a line and column.
    pub start: Position,
    /// What the path spells, as the layout places it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    /// The declaration the address reaches through re-exports, when that is
    /// somewhere else: `vvv_core::text::span::Span` for `use vvv_core::Span`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Address>,
    /// The file declaring what the import reaches — the origin's when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<RelPath>,
}

/// A file that imports from the one asked about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Importer {
    pub path: RelPath,
    #[serde(flatten)]
    pub import: ImportRef,
    pub start: Position,
}

/// A declaration as the graph holds it: placed, with its reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Placed {
    pub path: RelPath,
    #[serde(flatten)]
    pub symbol: Symbol,
    /// Where the declaration's name starts, as a line and column.
    pub start: Position,
    pub address: Address,
    pub reach: Reach,
}
