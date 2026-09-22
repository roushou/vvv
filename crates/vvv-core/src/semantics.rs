//! What a language's syntax means: the table that gives facts their sense.
//!
//! Grammar says what the tree calls things; semantics says what they do —
//! whether importing a module scopes its names, which declarations a path
//! can reach, what each visibility modifier lets see the declaration. All
//! data: a language module contributes one `Semantics` beside its `Grammar`.

use serde::{Deserialize, Serialize};

use crate::symbol::SymbolKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Semantics {
    /// Whether importing a module brings its names into scope on its own.
    /// `false` for Rust (`use a::b;` scopes `b`, not `b::X`; only `use
    /// a::b::*` does); `true` where the import grammar hides which names
    /// are taken, as with `import { X } from './b'` today.
    pub import_scopes_names: bool,
    /// Declarations a path can reach — structs, functions, modules. Methods,
    /// fields and variants are reached through a type, which syntax alone
    /// cannot follow, so only these can be a rename's target.
    pub addressable: &'static [SymbolKind],
    /// Each visibility modifier and what it lets see the declaration.
    pub visibility: &'static [VisibilityRule],
    /// What a declaration without a modifier lets see it.
    pub default_visibility: ReachKind,
}

impl Semantics {
    pub fn is_addressable(&self, kind: SymbolKind) -> bool {
        self.addressable.contains(&kind)
    }

    /// The reach a modifier grants, or the default when there is none.
    pub fn reach_kind(&self, modifier: Option<&str>) -> ReachKind {
        let Some(text) = modifier else {
            return self.default_visibility;
        };
        self.visibility
            .iter()
            .find(|rule| rule.matches(text))
            .map_or(self.default_visibility, |rule| rule.reach)
    }
}

/// "The modifier spelled `text` grants `reach`." Rules are tried in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityRule {
    pub text: &'static str,
    pub reach: ReachKind,
    /// Match by prefix (`pub(in` for `pub(in a::b)`) rather than exactly.
    pub prefix: bool,
}

impl VisibilityRule {
    pub const fn exact(text: &'static str, reach: ReachKind) -> Self {
        Self {
            text,
            reach,
            prefix: false,
        }
    }

    pub const fn prefix(text: &'static str, reach: ReachKind) -> Self {
        Self {
            text,
            reach,
            prefix: true,
        }
    }

    fn matches(&self, modifier: &str) -> bool {
        if self.prefix {
            modifier.starts_with(self.text)
        } else {
            modifier == self.text
        }
    }
}

/// What a visibility modifier means, relative to the declaring module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReachKind {
    /// The declaring module and its descendants (Rust private).
    Declaring,
    /// The parent module and its descendants (`pub(super)`).
    Parent,
    /// The whole package (`pub(crate)`).
    Package,
    /// Everyone (`pub`, `export`).
    Everyone,
    /// A module named in the modifier (`pub(in a::b)`); the layout resolves it.
    Path,
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST: Semantics = Semantics {
        import_scopes_names: false,
        addressable: &[SymbolKind::Struct],
        visibility: &[
            VisibilityRule::exact("pub", ReachKind::Everyone),
            VisibilityRule::exact("pub(crate)", ReachKind::Package),
            VisibilityRule::exact("pub(super)", ReachKind::Parent),
            VisibilityRule::exact("pub(self)", ReachKind::Declaring),
            VisibilityRule::prefix("pub(in", ReachKind::Path),
        ],
        default_visibility: ReachKind::Declaring,
    };

    #[test]
    fn modifiers_map_to_reach_kinds() {
        assert_eq!(RUST.reach_kind(None), ReachKind::Declaring);
        assert_eq!(RUST.reach_kind(Some("pub")), ReachKind::Everyone);
        assert_eq!(RUST.reach_kind(Some("pub(super)")), ReachKind::Parent);
        assert_eq!(RUST.reach_kind(Some("pub(in crate::a)")), ReachKind::Path);
        assert_eq!(
            RUST.reach_kind(Some("pub(weird)")),
            ReachKind::Declaring,
            "unknown: narrowest"
        );
    }
}
