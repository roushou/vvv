//! Language support: the parser adapter plus one module per language.
//!
//! [`syntax`] is the **only** module that touches `ast_grep_core` nodes; it
//! turns a language's declarative tables ([`vvv_core::SymbolRule`],
//! [`vvv_core::ImportGrammar`]) into data. Each language module contributes
//! a `grammar.rs` (its [`vvv_core::Grammar`] and [`vvv_core::Semantics`]), a
//! `layout.rs` ([`vvv_core::Layout`]: pure path algebra over a project) and a
//! `surgery.rs` ([`vvv_core::Surgery`]: text in, edits out), assembled into a
//! [`syntax::AstGrepLanguage`]. Language modules must not import
//! `ast_grep_core`, and nothing in this crate touches a file system.
//!
//! Languages are compiled in behind Cargo features (`rust`, `typescript`),
//! each enabling exactly one grammar of `ast-grep-language`.

pub mod syntax;

#[cfg(feature = "rust")]
pub mod rust;
#[cfg(feature = "typescript")]
pub mod typescript;
