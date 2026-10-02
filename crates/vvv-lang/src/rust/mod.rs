//! Rust support: grammar and semantics as tables, the Cargo layout as pure
//! path algebra, and the surgery that spells `use` paths and `mod` lines.

mod grammar;
mod layout;
mod surgery;

use vvv_core::LanguageId;

use crate::syntax::{AstGrepLanguage, NavigationSyntax};

pub use layout::RustLayout;
pub use surgery::RustSurgery;

/// Rust as a [`vvv_core::Language`]: the grammar and semantics tables, the
/// Cargo layout, and the surgery for `use` paths and `mod` lines.
pub type Rust = AstGrepLanguage<ast_grep_language::Rust>;

impl Rust {
    pub const ID: LanguageId = LanguageId::new("rust");
}

impl Default for Rust {
    fn default() -> Self {
        AstGrepLanguage::new(
            Self::ID,
            &["rs"],
            ast_grep_language::Rust,
            grammar::GRAMMAR,
            &grammar::SEMANTICS,
        )
        .with_navigation_syntax(NavigationSyntax::Rust)
        .with_layout(RustLayout)
        .with_surgery(RustSurgery)
    }
}

#[cfg(test)]
#[path = "tests/search.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/resolver.rs"]
mod resolver_tests;

#[cfg(test)]
#[path = "tests/regroup.rs"]
mod regroup_tests;

#[cfg(test)]
#[path = "tests/macro_scope.rs"]
mod macro_scope_tests;

#[cfg(test)]
#[path = "tests/construct_navigation.rs"]
mod construct_navigation_tests;
