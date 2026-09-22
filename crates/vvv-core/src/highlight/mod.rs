//! Syntax colouring as data. A grammar declares which node kinds are
//! strings, comments, types and so on ([`HighlightRule`]); a language returns
//! [`Highlight`] spans for a text; a display decides the colours.

use serde::{Deserialize, Serialize};

use crate::text::Span;

/// The classes a display distinguishes. Coarse on purpose: enough to read
/// code, not enough to argue about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HighlightKind {
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Function,
    Macro,
    Attribute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Highlight {
    pub span: Span,
    pub kind: HighlightKind,
}

/// "Nodes of kind `node` (under a parent of kind `under`, if given) are
/// `kind`." A matched node is coloured whole; nothing under it is visited.
///
/// Keywords need no rule: any anonymous node made of letters (`fn`, `pub`,
/// `import`) is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightRule {
    pub node: &'static str,
    pub under: Option<&'static str>,
    pub kind: HighlightKind,
}

impl HighlightRule {
    pub const fn new(node: &'static str, kind: HighlightKind) -> Self {
        Self {
            node,
            under: None,
            kind,
        }
    }

    pub const fn under(mut self, parent: &'static str) -> Self {
        self.under = Some(parent);
        self
    }
}
