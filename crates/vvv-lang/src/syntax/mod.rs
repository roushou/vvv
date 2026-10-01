//! Adapter from `ast-grep-core` to the data-only [`vvv_core::search`] model.
//!
//! Language modules own an [`AstGrepSearcher`] for their grammar and delegate
//! [`vvv_core::Language`] methods to it. Nothing from tree-sitter escapes this
//! module; languages contribute data (`SymbolRule` tables, an
//! `ImportGrammar`, identifier kinds) and select parser-side navigation syntax.

mod bindings;
mod calls;
mod compiled;
mod declarations;
#[cfg(test)]
pub(crate) mod fixture;
mod highlights;
mod imports;
mod language;
mod modules;
mod navigation;
#[cfg(feature = "rust")]
mod rust;
mod searcher;
mod signatures;
mod symbols;
#[cfg(feature = "typescript")]
mod typescript;
#[cfg(any(feature = "rust", feature = "typescript"))]
mod views;

pub use language::AstGrepLanguage;
pub use searcher::AstGrepSearcher;

pub use navigation::NavigationSyntax;
