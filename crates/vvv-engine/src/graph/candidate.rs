//! A file paired with the language that claims it: the unit every search,
//! rename and move puts its questions to.

use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use super::{Fragment, Namespace, Scope};
use crate::{Match, SourceFile};

use vvv_core::{Facts, ImportRef, Language, LanguageId, Query, SearchError};

use crate::EngineError;

/// Cheap to clone: the text and the facts are shared, so a model that keeps
/// files between questions hands them out without copying, and a file is
/// parsed once however many questions are asked of it.
#[derive(Clone)]
pub struct Candidate {
    file: Arc<SourceFile>,
    language: Arc<dyn Language>,
    facts: Arc<OnceLock<Result<Facts, SearchError>>>,
    /// The file's edges, resolved against a project; replaced when the
    /// project it was resolved against is gone.
    fragment: Arc<PerBuild<Fragment>>,
    /// What the file sees, read off the fragment; lives as long as it does.
    scope: Arc<PerBuild<Scope>>,
}

/// Something derived from the file against one build of its language's
/// project, kept until the project moves on.
struct PerBuild<T> {
    slot: RwLock<Option<(u64, Arc<T>)>>,
}

impl<T> PerBuild<T> {
    fn empty() -> Self {
        Self {
            slot: RwLock::new(None),
        }
    }

    /// The value for `generation`, building it when what is held is for
    /// another build or nothing is held yet.
    fn get_or_build(
        &self,
        generation: u64,
        build: impl FnOnce() -> Result<T, EngineError>,
    ) -> Result<Arc<T>, EngineError> {
        if let Some((_, value)) = self
            .slot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|(built, _)| *built == generation)
        {
            return Ok(value.clone());
        }
        let value = Arc::new(build()?);
        *self
            .slot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((generation, value.clone()));
        Ok(value)
    }
}

impl Candidate {
    pub fn new(file: SourceFile, language: Arc<dyn Language>) -> Self {
        Self {
            file: Arc::new(file),
            language,
            facts: Arc::new(OnceLock::new()),
            fragment: Arc::new(PerBuild::empty()),
            scope: Arc::new(PerBuild::empty()),
        }
    }

    /// The file's edges, resolved through `ns`; computed once per project
    /// build and shared by every clone.
    pub fn fragment(&self, ns: &Namespace) -> Result<Arc<Fragment>, EngineError> {
        self.fragment
            .get_or_build(ns.generation(), || Fragment::build(self, ns))
    }

    /// What the file can see — its imports as names and addresses — read
    /// off the fragment once per project build and shared by every clone.
    pub fn scope(&self, ns: &Namespace) -> Result<Arc<Scope>, EngineError> {
        self.scope.get_or_build(ns.generation(), || {
            let fragment = self.fragment(ns)?;
            Ok(Scope::of(ns, self.path(), &fragment))
        })
    }

    /// Everything the language can say about this file, from one parse.
    pub fn facts(&self) -> Result<&Facts, EngineError> {
        self.facts
            .get_or_init(|| self.language.facts(self.text()))
            .as_ref()
            .map_err(|source| self.failed(source.clone()))
    }

    pub fn file(&self) -> &SourceFile {
        &self.file
    }

    pub fn path(&self) -> &Path {
        self.file.path()
    }

    pub fn text(&self) -> &str {
        self.file.text()
    }

    pub fn language(&self) -> LanguageId {
        self.language.id()
    }

    /// Whether the text spells every word as a whole token — not as part of
    /// a longer identifier. A file that does not cannot match a query built
    /// from them, so this is asked before parsing.
    pub fn contains_all(&self, words: &[&str]) -> bool {
        let text = self.file.text();
        let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || !b.is_ascii();
        words.iter().all(|word| {
            text.match_indices(word).any(|(start, _)| {
                let end = start + word.len();
                let before = start.checked_sub(1).map(|i| text.as_bytes()[i]);
                let after = text.as_bytes().get(end).copied();
                !before.is_some_and(is_word) && !after.is_some_and(is_word)
            })
        })
    }

    /// Matches of `query` in this file, located.
    pub fn find(&self, query: &Query) -> Result<Vec<Match>, EngineError> {
        let raw = self
            .language
            .find(self.text(), query)
            .map_err(|source| self.failed(source))?;
        Ok(self.locate(raw))
    }

    /// Every identifier token spelling `name`, located.
    pub fn references(&self, name: &str) -> Result<Vec<Match>, EngineError> {
        let facts = self.facts()?;
        Ok(facts
            .tokens_named(name)
            .map(|(span, kind)| {
                let raw = vvv_core::RawMatch::plain(span, kind, &self.text()[span.start..span.end]);
                Match::locate(raw, &self.file, self.language.id())
            })
            .collect())
    }

    /// Import paths in this file, in source order.
    pub fn imports(&self) -> Result<Vec<ImportRef>, EngineError> {
        Ok(self.facts()?.imports.clone())
    }

    fn locate(&self, raw: Vec<vvv_core::RawMatch>) -> Vec<Match> {
        raw.into_iter()
            .map(|r| Match::locate(r, &self.file, self.language.id()))
            .collect()
    }

    fn failed(&self, source: SearchError) -> EngineError {
        EngineError::Search {
            path: self.path().into(),
            source,
        }
    }
}
