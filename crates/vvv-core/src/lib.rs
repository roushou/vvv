//! The plugin contract of `vvv`: what a language needs to be one.
//!
//! The nouns every crate shares (spans, positions, symbols, addresses), the
//! traits a language implements ([`lang::Language`], [`resolve::Layout`],
//! [`resolve::Surgery`]), what they are given ([`query::Query`],
//! [`resolve::Project`]) and what they hand back ([`search::RawMatch`],
//! [`facts::Facts`], [`edit::Edit`]). No parser, no file system, no
//! lifecycle: everything here is plain, serializable data, so a plugin can
//! move behind a process or WASM boundary without changing the model. What
//! the engine does with it lives in `vvv-engine`.
//!
//! A language is described by rule tables, and they read alike: a rule is
//! built by `new(..)` or a kind constructor, `under(kind)` scopes it to a
//! node whose direct parent has that kind, `within(kind)` to a node with
//! that ancestor, and the remaining builders (`field`, `leading`,
//! `visibility`) say where the rest of the declaration lives.

pub mod edit;
pub mod facts;
pub mod highlight;
pub mod import;
pub mod lang;
pub mod oracle;
pub mod paths;
pub mod query;
pub mod resolve;
pub mod search;
pub mod semantics;
pub mod symbol;
pub mod text;

pub use edit::{ChangeSet, Edit, EditConflict};
pub use facts::{Facts, Token};
pub use highlight::{Highlight, HighlightKind, HighlightRule};
pub use import::{ImportGrammar, ImportGroup, ImportNesting, ImportRef, ImportRule, ReExportRule};
pub use lang::{Grammar, Language, LanguageId, LanguageRegistry};
pub use oracle::{Oracle, Referent};
pub use paths::{ModulePath, Name, PathHead, PathSyntax, RelPath};
pub use query::{Query, QueryBuilder, QueryError};
pub use resolve::{
    Address, Dependency, FileSet, Layout, Package, PackageId, Packages, Parsed, Project, Regrouped,
    ResolveError, SideEdit, Surgery,
};
pub use search::{Capture, CaptureValue, RawMatch, Role, SearchError};
pub use semantics::{ReachKind, Semantics, VisibilityRule};
pub use symbol::{Modifier, ModifierAt, Symbol, SymbolKind, SymbolRule, UnknownSymbolKind};
pub use text::{Position, SourceText, Span};
