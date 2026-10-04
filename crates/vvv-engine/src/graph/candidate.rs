//! A file paired with the language that claims it: the unit every search,
//! rename and move puts its questions to.

use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use super::{Declared, Fragment, Namespace, Scope};
use crate::{Match, Placed, SourceFile};

use vvv_core::{Facts, Language, LanguageId, Project, Query, SearchError};

use crate::EngineError;

/// Cheap to clone: the text and the facts are shared, so a model that keeps
/// files between questions hands them out without copying, and a file is
/// parsed once however many questions are asked of it.
#[derive(Clone)]
pub struct Candidate {
    pub(super) source: Arc<SourceFacts>,
    /// The file's edges, resolved against a project; replaced when the
    /// project or consulted source versions change.
    fragment: Arc<PerBuild<Fragment>>,
    /// What the file sees, read off the fragment; lives as long as it does.
    scope: Arc<PerBuild<Scope>>,
}

/// A captured source and its shared parse, without derived graph caches.
/// Namespace snapshots retain these inputs without retaining candidates' scopes.
pub(super) struct SourceFacts {
    file: Arc<SourceFile>,
    language: Arc<dyn Language>,
    facts: OnceLock<Result<Facts, SearchError>>,
}

impl SourceFacts {
    pub(super) fn file(&self) -> &SourceFile {
        &self.file
    }
    pub(super) fn path(&self) -> &Path {
        self.file.path()
    }
    pub(super) fn facts(&self) -> Result<&Facts, EngineError> {
        self.facts
            .get_or_init(|| self.file.facts(self.language.as_ref()))
            .as_ref()
            .map_err(|source| EngineError::Search {
                path: self.path().into(),
                source: source.clone(),
            })
    }
}

/// Something derived from the file against one build of its language's
/// project, kept while its captured inputs remain current.
struct PerBuild<T> {
    slot: RwLock<Option<(Arc<Project>, Arc<T>)>>,
}

impl<T> PerBuild<T> {
    fn empty() -> Self {
        Self {
            slot: RwLock::new(None),
        }
    }

    /// The value for `project`, building it when what is held is for
    /// another build or nothing is held yet.
    fn get_or_build(
        &self,
        project: &Arc<Project>,
        valid: impl Fn(&T) -> bool,
        build: impl FnOnce() -> Result<T, EngineError>,
    ) -> Result<Arc<T>, EngineError> {
        if let Some((_, value)) = self
            .slot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|(built, value)| Arc::ptr_eq(built, project) && valid(value))
        {
            return Ok(value.clone());
        }
        let value = Arc::new(build()?);
        *self
            .slot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((project.clone(), value.clone()));
        Ok(value)
    }
}

impl Candidate {
    /// A fragment's declaration as an answer names it: with its file and line.
    pub(crate) fn placed(&self, declared: &Declared) -> Placed {
        Placed {
            path: self.path().into(),
            symbol: declared.symbol.clone(),
            start: self
                .file()
                .source()
                .position(declared.symbol.name_span.start),
            address: declared.address.clone(),
            reach: declared.reach.clone(),
        }
    }

    pub fn new(file: SourceFile, language: Arc<dyn Language>) -> Self {
        Self {
            source: Arc::new(SourceFacts {
                file: Arc::new(file),
                language,
                facts: OnceLock::new(),
            }),
            fragment: Arc::new(PerBuild::empty()),
            scope: Arc::new(PerBuild::empty()),
        }
    }

    /// The file's edges, resolved through `ns`; computed once per project
    /// build and shared by every clone.
    pub fn fragment(&self, ns: &Namespace) -> Result<Arc<Fragment>, EngineError> {
        self.fragment.get_or_build(
            ns.project(),
            |fragment| fragment.is_current(ns),
            || Fragment::build(&self.source, ns),
        )
    }

    /// What the file can see — its imports as names and addresses — read
    /// off the fragment once per project build and shared by every clone.
    pub fn scope(&self, ns: &Namespace) -> Result<Arc<Scope>, EngineError> {
        let fragment = self.fragment(ns)?;
        self.scope.get_or_build(
            ns.project(),
            |scope| scope.is_current(&fragment),
            || Ok(Scope::new(ns, self.path(), &fragment)),
        )
    }

    /// Everything the language can say about this file, from one parse.
    pub fn facts(&self) -> Result<&Facts, EngineError> {
        self.source.facts()
    }

    pub fn file(&self) -> &SourceFile {
        &self.source.file
    }

    pub fn path(&self) -> &Path {
        self.source.file.path()
    }

    pub fn text(&self) -> &str {
        self.source.file.text()
    }

    pub fn language(&self) -> LanguageId {
        self.source.language.id()
    }

    /// Whether the text spells every word as a whole token — not as part of
    /// a longer identifier. A file that does not cannot match a query built
    /// from them, so this is asked before parsing.
    pub fn contains_all(&self, words: &[&str]) -> bool {
        let text = self.source.file.text();
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
            .file()
            .find(self.source.language.as_ref(), query)
            .map_err(|source| self.failed(source))?;
        Ok(self.locate(raw))
    }

    /// Retained or client-supplied matches must still describe this snapshot
    /// before their coordinates or captures can authorize edits.
    pub(crate) fn validate_matches(
        &self,
        query: &Query,
        matches: &[Match],
    ) -> Result<(), EngineError> {
        if query.language().is_some_and(|id| *id != self.language()) {
            return Err(crate::ApplyError::Stale {
                path: self.path().into(),
            }
            .into());
        }
        let current = self.find(query)?;
        if matches
            .iter()
            .any(|held| !current.iter().any(|fresh| held.same_source_match(fresh)))
        {
            return Err(crate::ApplyError::Stale {
                path: self.path().into(),
            }
            .into());
        }
        Ok(())
    }

    /// Every identifier token spelling `name`, located.
    pub fn references(&self, name: &str) -> Result<Vec<Match>, EngineError> {
        let facts = self.facts()?;
        Ok(facts
            .tokens_named(name)
            .map(|(span, kind)| {
                let raw = vvv_core::RawMatch::plain(span, kind, &self.text()[span.start..span.end]);
                Match::locate(raw, &self.source.file, self.source.language.id())
            })
            .collect())
    }

    fn locate(&self, raw: Vec<vvv_core::RawMatch>) -> Vec<Match> {
        raw.into_iter()
            .map(|r| Match::locate(r, &self.source.file, self.source.language.id()))
            .collect()
    }

    fn failed(&self, source: SearchError) -> EngineError {
        EngineError::Search {
            path: self.path().into(),
            source,
        }
    }
}
