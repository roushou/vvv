//! Adapter from `ast-grep-core` to the data-only [`vvv_core::search`] model.
//!
//! Language modules own an [`AstGrepSearcher`] for their grammar and delegate
//! [`vvv_core::Language`] methods to it. Nothing from tree-sitter escapes this
//! module; languages contribute only data (`SymbolRule` tables, an
//! `ImportGrammar`, identifier kinds).

mod compiled;
#[cfg(test)]
pub(crate) mod fixture;
mod highlights;
mod imports;
mod language;
mod searcher;
mod symbols;

pub use language::AstGrepLanguage;
pub use searcher::AstGrepSearcher;
