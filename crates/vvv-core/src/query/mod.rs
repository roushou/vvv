//! What the user is looking for. Declarative and serializable so the CLI, a
//! TUI form, and a JSON request all build the same value.
//!
//! `pattern` and `kind` select nodes structurally; `symbol` and `name`
//! select declarations. When both sides are given a match must satisfy both.

use serde::{Deserialize, Serialize};

use crate::lang::LanguageId;
use crate::symbol::SymbolKind;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// ast-grep style pattern with meta-variables, e.g. `fn $NAME($$$ARGS) { $$$ }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pattern: Option<String>,
    /// Tree-sitter node kind, e.g. `function_item`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    /// Only declarations of this kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    symbol: Option<SymbolKind>,
    /// Only declarations with exactly this name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Restrict to one language; otherwise every registered language is searched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language: Option<LanguageId>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum QueryError {
    #[error("a query needs at least one of: pattern, kind, symbol, name")]
    Empty,
}

impl Query {
    pub fn builder() -> QueryBuilder {
        QueryBuilder::default()
    }

    /// A query has to ask something: one read from JSON may not.
    pub fn check(&self) -> Result<(), QueryError> {
        if self.is_structural() || self.is_symbolic() {
            Ok(())
        } else {
            Err(QueryError::Empty)
        }
    }

    pub fn pattern(pattern: impl Into<String>) -> Self {
        Self {
            pattern: Some(pattern.into()),
            ..Self::default()
        }
    }

    pub fn of_kind(kind: impl Into<String>) -> Self {
        Self {
            kind: Some(kind.into()),
            ..Self::default()
        }
    }

    pub fn of_symbol(symbol: SymbolKind) -> Self {
        Self {
            symbol: Some(symbol),
            ..Self::default()
        }
    }

    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::default()
        }
    }

    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    pub fn with_symbol(mut self, symbol: SymbolKind) -> Self {
        self.symbol = Some(symbol);
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn in_language(mut self, language: impl Into<LanguageId>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn pattern_str(&self) -> Option<&str> {
        self.pattern.as_deref()
    }

    pub fn kind_str(&self) -> Option<&str> {
        self.kind.as_deref()
    }

    pub fn symbol(&self) -> Option<SymbolKind> {
        self.symbol
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn language(&self) -> Option<&LanguageId> {
        self.language.as_ref()
    }

    /// True when the query selects nodes structurally (pattern or kind).
    pub fn is_structural(&self) -> bool {
        self.pattern.is_some() || self.kind.is_some()
    }

    /// True when the query filters on declarations (symbol or name).
    pub fn is_symbolic(&self) -> bool {
        self.symbol.is_some() || self.name.is_some()
    }

    /// Words every match must contain as whole tokens: the identifier- and
    /// number-like runs of the pattern that are not meta-variables, and the
    /// symbol name. A file whose text lacks any of them cannot match, so a
    /// search may skip it without parsing. Empty for a kind-only query.
    pub fn literals(&self) -> Vec<&str> {
        let mut words: Vec<&str> = self
            .pattern
            .as_deref()
            .map(Self::pattern_words)
            .unwrap_or_default();
        words.extend(self.name.as_deref());
        words.sort_unstable();
        words.dedup();
        words
    }

    /// Token-like runs of `pattern`, skipping any introduced by `$`.
    fn pattern_words(pattern: &str) -> Vec<&str> {
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut words = Vec::new();
        let mut rest = pattern;
        while let Some(start) = rest.find(is_word) {
            let meta = rest[..start].ends_with('$');
            let tail = &rest[start..];
            let end = tail.find(|c| !is_word(c)).unwrap_or(tail.len());
            if !meta {
                words.push(&tail[..end]);
            }
            rest = &tail[end..];
        }
        words
    }
}

/// Assemble a [`Query`] from optional parts (typically CLI flags).
#[derive(Debug, Default)]
pub struct QueryBuilder {
    query: Query,
}

impl QueryBuilder {
    pub fn pattern(mut self, pattern: Option<impl Into<String>>) -> Self {
        self.query.pattern = pattern.map(Into::into);
        self
    }

    pub fn kind(mut self, kind: Option<impl Into<String>>) -> Self {
        self.query.kind = kind.map(Into::into);
        self
    }

    pub fn symbol(mut self, symbol: Option<SymbolKind>) -> Self {
        self.query.symbol = symbol;
        self
    }

    pub fn name(mut self, name: Option<impl Into<String>>) -> Self {
        self.query.name = name.map(Into::into);
        self
    }

    pub fn language(mut self, language: Option<impl Into<LanguageId>>) -> Self {
        self.query.language = language.map(Into::into);
        self
    }

    pub fn build(self) -> Result<Query, QueryError> {
        self.query.check()?;
        Ok(self.query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_pattern_words_minus_meta_variables() {
        let q = Query::pattern("$A.foo($$$ARGS, bar_2, $_)");
        assert_eq!(q.literals(), vec!["bar_2", "foo"]);
    }

    #[test]
    fn literals_include_the_name_and_skip_kinds() {
        assert_eq!(Query::of_kind("mod_item").literals(), Vec::<&str>::new());
        assert_eq!(
            Query::of_kind("fn").with_name("run").literals(),
            vec!["run"]
        );
        assert_eq!(
            Query::pattern("run").with_name("run").literals(),
            vec!["run"]
        );
    }

    #[test]
    fn literals_keep_number_tokens_whole() {
        assert_eq!(Query::pattern("$X + 0x1f").literals(), vec!["0x1f"]);
    }

    #[test]
    fn literals_keep_keywords_and_unicode() {
        assert_eq!(
            Query::pattern("fn $F() { héllo }").literals(),
            vec!["fn", "héllo"]
        );
    }
}
