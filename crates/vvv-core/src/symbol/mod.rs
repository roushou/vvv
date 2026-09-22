//! Language-neutral view of declarations.
//!
//! A plugin describes its grammar with [`SymbolRule`]s (data); the generic
//! extractor in `vvv-lang` turns them into [`Symbol`]s. Nothing here knows
//! tree-sitter.

mod kind;

use serde::{Deserialize, Serialize};

pub use kind::{SymbolKind, UnknownSymbolKind};

use crate::text::Span;

/// A declaration found in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    /// The identifier token itself: what a rename replaces.
    pub name_span: Span,
    /// The declaration node.
    pub span: Span,
    /// The declaration with what belongs to it in the text: leading
    /// attributes and doc comments, an enclosing `export`. What moving the
    /// declaration cuts; what an outline shows.
    pub extent: Span,
    /// The visibility as written (`pub(crate)`, `export`); `None` when the
    /// declaration has no modifier. What it means is the language's
    /// [`Semantics`](crate::Semantics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Modifier>,
}

impl Symbol {
    /// A symbol whose extent is its node and which has no modifier.
    pub fn plain(kind: SymbolKind, name: impl Into<String>, name_span: Span, span: Span) -> Self {
        Self {
            kind,
            name: name.into(),
            name_span,
            span,
            extent: span,
            visibility: None,
        }
    }

    pub fn modifier(&self) -> Option<&str> {
        self.visibility.as_ref().map(|m| m.text.as_str())
    }
}

/// A visibility modifier as it appears in the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifier {
    pub span: Span,
    pub text: String,
}

/// Where a declaration's visibility modifier is found in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierAt {
    /// A direct child of this kind (`visibility_modifier` in Rust).
    Child(&'static str),
    /// An enclosing node of this kind whose first token is the modifier
    /// (`export_statement` in TypeScript). It becomes part of the extent.
    Parent(&'static str),
}

/// "Nodes of kind `node` declare a `kind` whose name is the field `name_field`."
///
/// Rules are tried in order; the first whose `node`, `under` and `within`
/// all match wins, so put the more specific rule (a method inside an impl)
/// before the general one (a function).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolRule {
    pub node: &'static str,
    /// Field holding the identifier; `None` when the node is its own name
    /// (a bare enum member, say).
    pub name_field: Option<&'static str>,
    pub kind: SymbolKind,
    /// Only applies when the direct parent has this kind.
    pub under: Option<&'static str>,
    /// Only applies when an ancestor of this node kind exists, without a
    /// nested declaration of the same `node` kind in between.
    pub within: Option<&'static str>,
    /// Kinds of preceding siblings that belong to the declaration
    /// (attributes, doc comments) and extend its extent, as long as no blank
    /// line separates them from it.
    pub leading: &'static [&'static str],
    /// Where the visibility modifier is, if the language has one.
    pub visibility: Option<ModifierAt>,
    /// When the name node has a child in this field, the name is that
    /// child's, repeatedly: `Foo` inside the `generic_type` `Foo<'a>` an
    /// `impl` is for.
    pub name_inner: Option<&'static str>,
}

impl SymbolRule {
    pub const fn new(node: &'static str, name_field: &'static str, kind: SymbolKind) -> Self {
        Self {
            node,
            name_field: Some(name_field),
            kind,
            under: None,
            within: None,
            leading: &[],
            visibility: None,
            name_inner: None,
        }
    }

    /// The node is the identifier token itself.
    pub const fn self_named(node: &'static str, kind: SymbolKind) -> Self {
        Self {
            node,
            name_field: None,
            kind,
            under: None,
            within: None,
            leading: &[],
            visibility: None,
            name_inner: None,
        }
    }

    pub const fn name_inner(mut self, field: &'static str) -> Self {
        self.name_inner = Some(field);
        self
    }

    pub const fn leading(mut self, kinds: &'static [&'static str]) -> Self {
        self.leading = kinds;
        self
    }

    pub const fn visibility(mut self, at: ModifierAt) -> Self {
        self.visibility = Some(at);
        self
    }

    /// The direct parent must be this kind.
    pub const fn under(mut self, parent: &'static str) -> Self {
        self.under = Some(parent);
        self
    }

    pub const fn within(mut self, ancestor: &'static str) -> Self {
        self.within = Some(ancestor);
        self
    }
}
