//! What a search answers with: a [`RawMatch`] tied to its file as a
//! [`Match`] with a stable [`MatchId`], and, for a rename, each occurrence
//! judged against the declaration meant.

use std::collections::BTreeMap;
use std::fmt;

use vvv_core::RelPath;

use serde::{Deserialize, Serialize};
use vvv_core::{Address, CaptureValue, LanguageId, Position, RawMatch, Role, Span, Symbol};

use crate::SourceFile;

/// How sure a rename is that an occurrence refers to its target declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Declared, imported, or reached by a path that resolves to the target.
    Resolved,
    /// A bare token with no evidence either way.
    Unresolved,
    /// Resolves to a different declaration of the same name.
    Other,
}

/// What the file's imports and paths said about an occurrence: the ground
/// for its [`Confidence`]. Views tag `?` and `✗` sections with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// Written in the module that declares the target.
    Declaring,
    /// An import resolving to the target brings the name in.
    Imported,
    /// The tail of a qualified path resolving to the target.
    Path,
    /// A glob (or whole-file) import opens the declaring module.
    Opened,
    /// Reached through a re-export (`use vvv_core::Span` for
    /// `vvv_core::text::span::Span`) that vvv followed to the declaration.
    ReExport,
    /// Syntax could not place it; an oracle — a build, a language server —
    /// said it is the declaration.
    Oracle,
    /// A bare name nothing imports, or a path whose head cannot be placed.
    Unresolved,
    /// No declaration to judge against: a method or field, reached through
    /// a type; the token merely spells the name.
    ByName,
    /// Resolves to another declaration of the same name in the workspace.
    OtherDeclaration,
    /// Resolves into a package outside the workspace.
    External,
    /// Syntax could not place it; an oracle said it is another declaration.
    OracleOther,
}

impl Reason {
    pub fn confidence(self) -> Confidence {
        match self {
            Self::Declaring
            | Self::Imported
            | Self::Path
            | Self::Opened
            | Self::ReExport
            | Self::Oracle => Confidence::Resolved,
            Self::Unresolved | Self::ByName => Confidence::Unresolved,
            Self::OtherDeclaration | Self::External | Self::OracleOther => Confidence::Other,
        }
    }
}

/// A token spelling the renamed name, with what the imports could tell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    #[serde(flatten)]
    pub m: Match,
    pub confidence: Confidence,
    #[serde(default = "Reason::unresolved")]
    pub reason: Reason,
}

impl Occurrence {
    pub fn judged(m: Match, reason: Reason) -> Self {
        Self {
            m,
            confidence: reason.confidence(),
            reason,
        }
    }
}

impl Reason {
    fn unresolved() -> Self {
        Self::Unresolved
    }
}
/// A language a search left out, and why: its grammar could not compile the
/// query (a Rust pattern on a TypeScript tree). Never fatal; the other
/// languages' matches are complete. Data only; the display layer words it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    pub language: LanguageId,
    pub reason: String,
}

/// Content-derived identifier: the same file, range and text yield the same id
/// across runs, so a selection made from one `search` can drive a later command.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MatchId(String);

impl MatchId {
    const LEN: usize = 12;

    /// The file, the span and the text, in one workspace spelling: an id is
    /// the same on every platform, so a selection made in one process holds
    /// in the next.
    pub fn derive(path: &RelPath, span: Span, text: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(path.as_str().as_bytes());
        hasher.update(&span.start.to_le_bytes());
        hasher.update(&span.end.to_le_bytes());
        hasher.update(text.as_bytes());
        let hex = hasher.finalize().to_hex();
        Self(hex[..Self::LEN].to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for MatchId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl fmt::Display for MatchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for MatchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MatchId({})", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub id: MatchId,
    pub path: RelPath,
    pub language: LanguageId,
    pub span: Span,
    pub start: Position,
    pub end: Position,
    pub kind: String,
    pub text: String,
    /// The source line on which the match starts, for display in context.
    #[serde(default)]
    pub line: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub captures: BTreeMap<String, CaptureValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Symbol>,
    #[serde(default)]
    pub role: Role,
    /// For a declaration in an addressable file: its module address, the
    /// value `outline` gives it. Filled by the engine, which has the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
}

impl Match {
    pub fn locate(raw: RawMatch, file: &SourceFile, language: LanguageId) -> Self {
        let source = file.source();
        let start = source.position(raw.span.start);
        let path = RelPath::from(file.path());
        Self {
            id: MatchId::derive(&path, raw.span, &raw.text),
            path,
            language,
            span: raw.span,
            start,
            end: source.position(raw.span.end),
            line: source
                .line(start.line as usize)
                .unwrap_or_default()
                .to_owned(),
            kind: raw.kind,
            text: raw.text,
            captures: raw.captures,
            symbol: raw.symbol,
            role: raw.role,
            address: None,
        }
    }

    pub fn capture(&self, name: &str) -> Option<&CaptureValue> {
        self.captures.get(name)
    }

    /// Number of lines the match spans.
    pub fn line_count(&self) -> u32 {
        self.end.line - self.start.line + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_id_is_stable_and_sensitive() {
        let a = MatchId::derive(&RelPath::from("a.rs"), Span::new(0, 3), "foo");
        let b = MatchId::derive(&RelPath::from("a.rs"), Span::new(0, 3), "foo");
        let c = MatchId::derive(&RelPath::from("b.rs"), Span::new(0, 3), "foo");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.as_str().len(), MatchId::LEN);
    }

    /// Occurrences sort by reason, and a preview lists `✓`, then `?`, then
    /// `✗`: the reasons must be declared in that order.
    #[test]
    fn reasons_are_declared_in_confidence_order() {
        let all = [
            Reason::Declaring,
            Reason::Imported,
            Reason::Path,
            Reason::Opened,
            Reason::ReExport,
            Reason::Oracle,
            Reason::Unresolved,
            Reason::ByName,
            Reason::OtherDeclaration,
            Reason::External,
            Reason::OracleOther,
        ];
        let confidences: Vec<Confidence> = all.iter().map(|r| r.confidence()).collect();
        let mut sorted = confidences.clone();
        sorted.sort();
        assert_eq!(confidences, sorted);
        assert!(all.windows(2).all(|w| w[0] < w[1]));
    }
}
