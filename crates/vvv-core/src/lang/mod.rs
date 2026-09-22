//! The language plugin boundary.
//!
//! A [`Language`] knows how to look inside one kind of source file. It never
//! hands out parser nodes: everything it returns is plain data ([`RawMatch`],
//! [`Symbol`], [`ImportRef`], [`Facts`]), which is what lets the core and the
//! planners be tested with a fake language and lets a real language move out
//! of process.

mod registry;

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};

pub use registry::LanguageRegistry;

use crate::facts::Facts;
use crate::highlight::{Highlight, HighlightRule};
use crate::import::{ImportGrammar, ImportRef};
use crate::paths::PathSyntax;
use crate::query::Query;
use crate::resolve::{Layout, Surgery};
use crate::search::{RawMatch, SearchError};
use crate::semantics::Semantics;
use crate::symbol::{Symbol, SymbolRule};

/// Stable identifier of a language (`rust`, `typescript`, ...).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LanguageId(Cow<'static, str>);

impl LanguageId {
    pub const fn new(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for LanguageId {
    fn from(id: String) -> Self {
        Self(Cow::Owned(id))
    }
}

impl From<&str> for LanguageId {
    fn from(id: &str) -> Self {
        Self(Cow::Owned(id.to_owned()))
    }
}

impl fmt::Display for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LanguageId({})", self.0)
    }
}

/// Everything a grammar-backed language declares about itself, as data:
/// what a declaration is, what an identifier token is, where import paths
/// live. A language module contributes one of these beside its semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grammar {
    pub symbols: &'static [SymbolRule],
    pub identifiers: &'static [&'static str],
    pub imports: ImportGrammar,
    pub highlights: &'static [HighlightRule],
}

impl Grammar {
    pub const EMPTY: Self = Self {
        symbols: &[],
        identifiers: &[],
        imports: ImportGrammar::EMPTY,
        highlights: &[],
    };
}

pub trait Language: Send + Sync {
    fn id(&self) -> LanguageId;

    /// File extensions (without the dot) this language claims.
    fn extensions(&self) -> &'static [&'static str];

    /// What the language's syntax means.
    fn semantics(&self) -> &'static Semantics;

    /// How the language spells paths: what its import text is parsed with
    /// and what a rendered path is spelled back with.
    fn paths(&self) -> PathSyntax;

    /// Text every glob import contains (`::*` in Rust), so a file that does
    /// not spell it need not be parsed when only globs are looked for;
    /// `None` when the language has no glob imports.
    fn glob_marker(&self) -> Option<&'static str> {
        None
    }

    /// Everything about one file from one parse.
    fn facts(&self, source: &str) -> Result<Facts, SearchError>;

    /// Whether this language can run `query` at all: its grammar compiles
    /// the pattern and knows the node kind. Asked once per language before
    /// any file is read; a language that declines is skipped, not fatal.
    fn accepts(&self, query: &Query) -> Result<(), SearchError> {
        let _ = query;
        Ok(())
    }

    /// Search over one file's text, honouring every part of the query.
    fn find(&self, source: &str, query: &Query) -> Result<Vec<RawMatch>, SearchError>;

    /// Declarations in one file, in source order.
    fn symbols(&self, source: &str) -> Result<Vec<Symbol>, SearchError> {
        let _ = source;
        Ok(Vec::new())
    }

    /// Every identifier token spelling `name`, declarations included.
    fn references(&self, source: &str, name: &str) -> Result<Vec<RawMatch>, SearchError> {
        let _ = (source, name);
        Ok(Vec::new())
    }

    /// Import paths in one file, in source order.
    fn imports(&self, source: &str) -> Result<Vec<ImportRef>, SearchError> {
        let _ = source;
        Ok(Vec::new())
    }

    /// Syntax colouring for one file, as non-overlapping spans in order.
    fn highlights(&self, source: &str) -> Result<Vec<Highlight>, SearchError> {
        let _ = source;
        Ok(Vec::new())
    }

    /// How the language's projects are arranged, when vvv can follow them.
    fn layout(&self) -> Option<&dyn Layout> {
        None
    }

    /// How to spell edits in the language, when files can be moved.
    fn surgery(&self) -> Option<&dyn Surgery> {
        None
    }
}
