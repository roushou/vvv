//! A language's module system on this tree.
//!
//! A [`Layout`] answers questions about a [`Project`]; a [`Surgery`] spells
//! edits in the language; the [`Semantics`] say what a modifier means. A
//! [`Namespace`] is the three bound to one language and the project the
//! graph built for it, so a command asks `namespace.address(path)` instead
//! of fetching the language, its layout and its project and threading them
//! through every call. It exists only for a language with a layout: a
//! language without one has no addresses to speak of.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vvv_core::{
    Address, Language, LanguageId, Layout, Project, ResolveError, Semantics, Surgery, Symbol,
    SymbolKind,
};

use crate::{EngineError, Reach};

#[derive(Clone)]
pub struct Namespace {
    language: Arc<dyn Language>,
    project: Arc<Project>,
    /// Which build of the project this is: what a fragment was resolved
    /// against, so a rebuilt project invalidates it.
    generation: u64,
}

impl Namespace {
    /// `language` must have a layout; the graph checks before building one.
    pub(crate) fn new(language: Arc<dyn Language>, project: Arc<Project>, generation: u64) -> Self {
        Self {
            language,
            project,
            generation,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn id(&self) -> LanguageId {
        self.language.id()
    }

    pub fn language(&self) -> &Arc<dyn Language> {
        &self.language
    }

    pub fn layout(&self) -> &dyn Layout {
        self.language
            .layout()
            .expect("a namespace exists only for a language with a layout")
    }

    /// How the language spells edits; a language that can be addressed but
    /// not yet edited is an error the caller names.
    pub fn surgery(&self) -> Result<&dyn Surgery, EngineError> {
        self.language
            .surgery()
            .ok_or_else(|| EngineError::NoLayout(self.id()))
    }

    pub fn semantics(&self) -> &'static Semantics {
        self.language.semantics()
    }

    pub fn project(&self) -> &Arc<Project> {
        &self.project
    }

    /// The module address of a file.
    pub fn address(&self, path: &Path) -> Result<Address, ResolveError> {
        self.layout().address(&self.project, path)
    }

    /// What an import path written in `from` points at.
    pub fn resolve(&self, from: &Path, import: &vvv_core::ModulePath) -> Option<Address> {
        self.layout().resolve(&self.project, from, import)
    }

    /// Whether a path can reach a declaration of this kind.
    pub fn is_addressable(&self, kind: SymbolKind) -> bool {
        self.semantics().is_addressable(kind)
    }

    /// The address of `symbol` declared in `module`, when a path can reach it.
    pub fn address_of(&self, module: &Address, symbol: &Symbol) -> Option<Address> {
        self.is_addressable(symbol.kind)
            .then(|| module.join(symbol.name.as_str()))
    }

    /// Who may name `symbol`, declared in `module`, from its modifier.
    pub fn reach(&self, module: &Address, symbol: &Symbol) -> Reach {
        Reach::of(self.semantics().reach_kind(symbol.modifier()), module, None)
    }

    /// The file declaring `address`, walking up when the address names an
    /// item inside a module.
    pub fn file_of(&self, address: &Address) -> Option<PathBuf> {
        let mut at = Some(address.clone());
        while let Some(a) = at {
            if let Some(file) = self
                .layout()
                .candidates(&self.project, &a)
                .into_iter()
                .find(|p| self.project.files.contains(p))
            {
                return Some(file);
            }
            at = a.parent();
        }
        None
    }
}
