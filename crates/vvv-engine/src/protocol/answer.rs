//! What a question about the tree comes back as: plain data over the facts
//! the model holds, for the CLI to print, the JSON to carry, an agent to read.

use vvv_core::RelPath;

use serde::{Deserialize, Serialize};

use crate::{Match, Occurrence, Reach, Skipped};
use vvv_core::{Address, Highlight, ImportRef, Position, Symbol};

/// `outline <path>`: what a file declares, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outline {
    pub path: RelPath,
    /// The file's own module address, when the language has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<Address>,
    pub items: Vec<OutlineItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineItem {
    #[serde(flatten)]
    pub symbol: Symbol,
    /// Where the extent starts and ends, as lines and columns.
    pub start: Position,
    pub end: Position,
    /// The module address the declaration is reached by, when a path can
    /// reach it in this language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    /// Who may name it, from its modifier and its module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<Reach>,
}

/// `references <name>`: the declarations called `name` and every token that
/// spells it, each judged against the declaration meant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct References {
    pub name: String,
    pub declarations: Vec<Match>,
    pub occurrences: Vec<Occurrence>,
}

/// `where <name>`: where `name` is declared and how to reach each site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locations {
    pub name: String,
    pub sites: Vec<Site>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    pub declaration: Match,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    /// The import statement that brings it into the file asked from, spelled
    /// as that language writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<String>,
}

/// `deps <path>`: what a file imports and who imports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deps {
    pub path: RelPath,
    /// The file's own module address, when the language has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<Address>,
    pub imports: Vec<Dep>,
    pub importers: Vec<Importer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
}

/// One import statement in the file and, when the layout can follow it,
/// where it leads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct Importer {
    pub path: RelPath,
    #[serde(flatten)]
    pub import: ImportRef,
    pub start: Position,
}

/// `explain <path>:<line>:<column>`: what is at a position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    pub path: RelPath,
    pub position: Position,
    /// The innermost declaration whose extent contains the position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Symbol>,
    /// Where that declaration's name starts, and the source line holding it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<Position>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// The file's module address, and the declaration's when a path reaches it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<Address>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<Reach>,
    /// Every other address a re-export chain offers the declaration at.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<Address>,
    /// The import statement at the position, when it is inside one: what
    /// it spells and where that really comes from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<Dep>,
    /// Files whose imports resolve to the declaration, to any address a
    /// re-export offers it at, or to its module when the declaration itself
    /// is not addressable.
    pub importers: Vec<RelPath>,
}

/// `Engine::file`: one file as it is, with its syntax colouring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub path: RelPath,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<Highlight>,
}

/// A declaration as the graph holds it: placed, with its reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    pub path: RelPath,
    #[serde(flatten)]
    pub symbol: Symbol,
    /// Where the declaration's name starts, as a line and column.
    pub start: Position,
    pub address: Address,
    pub reach: Reach,
}

/// `surface [package]`: what a package offers to everyone, and who takes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Surface {
    /// The package asked about; every package when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<vvv_core::PackageId>,
    pub items: Vec<Exposed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exposed {
    #[serde(flatten)]
    pub declaration: Placed,
    /// The other addresses it is offered at, through re-exports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<Address>,
    /// Files in the workspace importing it, at any of its addresses.
    pub importers: usize,
}

/// `impact <name>`: every module that would feel a change to a declaration —
/// those importing it, then those importing them, outward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impact {
    pub name: String,
    pub address: Address,
    /// Nearest first: depth 1 imports the declaration itself.
    pub consumers: Vec<Consumer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consumer {
    pub module: Address,
    pub path: RelPath,
    pub depth: u32,
    /// The module it reaches the declaration through; the declaration's
    /// own module at depth 1.
    pub through: Address,
}

/// `dead [--lang]`: declarations nothing in the workspace refers to, with
/// how many tokens might.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dead {
    pub items: Vec<Unreferenced>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unreferenced {
    #[serde(flatten)]
    pub declaration: Placed,
    /// Tokens spelling the name that vvv could not judge: any of them might
    /// be a use.
    pub unsure: usize,
}

/// `imports [path]`: import statements worth a look — unused, unresolved,
/// or the same target twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportsReport {
    /// The file asked about; every file when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<RelPath>,
    /// Declared imports whose bound name the file never spells again.
    pub unused: Vec<ImportSite>,
    /// Declared imports the layout could not place.
    pub unresolved: Vec<ImportSite>,
    /// Declared imports naming an address another statement in the file
    /// already brings in.
    pub redundant: Vec<ImportSite>,
    /// Files whose language has a layout but which it cannot place — a
    /// crate's integration tests, say — so their imports were not judged.
    pub unplaced: Vec<RelPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSite {
    pub path: RelPath,
    #[serde(flatten)]
    pub import: ImportRef,
    pub start: Position,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
}
