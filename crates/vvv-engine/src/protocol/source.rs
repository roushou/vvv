//! Versioned source locations shared by search and navigation.
use serde::{Deserialize, Serialize};
use vvv_core::{LanguageId, RelPath, Span, SymbolKind};

/// Digest of the complete UTF-8 source text, independent of its path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentId(String);

impl ContentId {
    pub(crate) fn from_fingerprint(value: &crate::plan::Fingerprint) -> Self {
        Self(value.as_str().to_owned())
    }

    pub fn of(text: &str) -> Self {
        Self(crate::plan::Fingerprint::of(text).as_str().to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAnchor {
    pub path: RelPath,
    pub content: ContentId,
    pub span: Span,
}

/// A declaration in one source version, not an identity across edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolRef {
    pub language: LanguageId,
    pub declaration: SourceAnchor,
    pub name_span: Span,
    pub kind: SymbolKind,
}
