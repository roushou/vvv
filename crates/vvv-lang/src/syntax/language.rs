use vvv_core::{
    Facts, Grammar, Highlight, ImportRef, Language, LanguageId, Layout, PathSyntax, Query,
    RawMatch, SearchError, Semantics, Surgery, Symbol,
};

use std::sync::Arc;

use super::AstGrepSearcher;
use ast_grep_core::tree_sitter::LanguageExt;

/// A [`Language`] entirely described by data and pure parts: an id, the
/// extensions it claims, a [`Grammar`] and its [`Semantics`], and optionally
/// a [`Layout`] and a [`Surgery`]. Every grammar-backed language is one of
/// these; a language module is the constructor plus its tables.
#[derive(Clone)]
pub struct AstGrepLanguage<L> {
    id: LanguageId,
    extensions: &'static [&'static str],
    semantics: &'static Semantics,
    searcher: AstGrepSearcher<L>,
    layout: Option<Arc<dyn Layout>>,
    surgery: Option<Arc<dyn Surgery>>,
}

impl<L> std::fmt::Debug for AstGrepLanguage<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AstGrepLanguage")
            .field("id", &self.id)
            .field("extensions", &self.extensions)
            .field("layout", &self.layout.is_some())
            .field("surgery", &self.surgery.is_some())
            .finish()
    }
}

impl<L: LanguageExt> AstGrepLanguage<L> {
    /// Describe a language; each language module wraps this in its own `new`.
    pub fn describe(
        id: LanguageId,
        extensions: &'static [&'static str],
        lang: L,
        grammar: Grammar,
        semantics: &'static Semantics,
    ) -> Self {
        Self {
            id,
            extensions,
            semantics,
            searcher: AstGrepSearcher::new(lang, grammar),
            layout: None,
            surgery: None,
        }
    }

    pub fn with_layout(mut self, layout: impl Layout + 'static) -> Self {
        self.layout = Some(Arc::new(layout));
        self
    }

    pub fn with_surgery(mut self, surgery: impl Surgery + 'static) -> Self {
        self.surgery = Some(Arc::new(surgery));
        self
    }

    pub fn searcher(&self) -> &AstGrepSearcher<L> {
        &self.searcher
    }
}

impl<L: LanguageExt + Send + Sync + 'static> Language for AstGrepLanguage<L> {
    fn id(&self) -> LanguageId {
        self.id.clone()
    }

    fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    fn semantics(&self) -> &'static Semantics {
        self.semantics
    }

    fn paths(&self) -> PathSyntax {
        self.searcher.paths()
    }

    fn glob_marker(&self) -> Option<&'static str> {
        self.searcher.glob_marker()
    }

    fn facts(&self, source: &str) -> Result<Facts, SearchError> {
        self.searcher.facts(source)
    }

    fn accepts(&self, query: &Query) -> Result<(), SearchError> {
        self.searcher.accepts(query)
    }

    fn find(&self, source: &str, query: &Query) -> Result<Vec<RawMatch>, SearchError> {
        self.searcher.find(source, query)
    }

    fn symbols(&self, source: &str) -> Result<Vec<Symbol>, SearchError> {
        Ok(self.searcher.symbols(source))
    }

    fn references(&self, source: &str, name: &str) -> Result<Vec<RawMatch>, SearchError> {
        Ok(self.searcher.references(source, name))
    }

    fn imports(&self, source: &str) -> Result<Vec<ImportRef>, SearchError> {
        self.searcher.imports(source)
    }

    fn highlights(&self, source: &str) -> Result<Vec<Highlight>, SearchError> {
        Ok(self.searcher.highlights(source))
    }

    fn layout(&self) -> Option<&dyn Layout> {
        self.layout.as_deref()
    }

    fn surgery(&self) -> Option<&dyn Surgery> {
        self.surgery.as_deref()
    }
}
