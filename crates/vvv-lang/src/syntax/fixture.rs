//! A project fixture for layout and surgery tests: files as text, from which
//! the [`Project`] a layout sees and the parsed files a surgery needs are
//! built — no file system, like the language itself.

// A language's tests call the subset its layout and surgery answer: TypeScript
// has no relocation, so its tests never call `relocate`. Which helpers a build
// uses therefore depends on the language features compiled in.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use vvv_core::{
    Address, Facts, ImportRef, Language, Layout, ModulePath, Packages, Parsed, PathSyntax, Project,
    ReachKind, Regrouped, ResolveError, SideEdit, SourceText, Surgery,
};

pub struct Fixture<'l> {
    language: &'l dyn Language,
    files: BTreeMap<PathBuf, String>,
    project: Project,
}

impl<'l> Fixture<'l> {
    /// Workspace-relative paths and their contents; manifests among them
    /// declare the packages.
    pub fn new(language: &'l dyn Language, files: &[(&str, &str)]) -> Self {
        let files: BTreeMap<PathBuf, String> = files
            .iter()
            .map(|(p, text)| (PathBuf::from(p), (*text).to_owned()))
            .collect();
        let layout = language.layout().expect("fixture language has a layout");
        let packages = Packages::new(files.iter().filter_map(|(path, text)| {
            let name = path.file_name()?;
            layout
                .manifests()
                .iter()
                .any(|m| name == *m)
                .then(|| layout.package(path, text))
                .flatten()
        }));
        let project = Project {
            packages,
            files: files.keys().cloned().collect(),
        };
        Self {
            language,
            files,
            project,
        }
    }

    fn layout(&self) -> &dyn Layout {
        self.language.layout().expect("layout")
    }

    fn surgery(&self) -> &dyn Surgery {
        self.language.surgery().expect("surgery")
    }

    pub fn address(&self, path: &Path) -> Result<Address, ResolveError> {
        self.layout().address(&self.project, path)
    }

    fn syntax(&self) -> PathSyntax {
        self.language.paths()
    }

    /// `import` as the language would have written it, resolved from `from`.
    pub fn resolve(&self, from: &Path, import: &str) -> Option<Address> {
        self.layout()
            .resolve(&self.project, from, &self.syntax().parse(import))
    }

    pub fn resolve_path(&self, from: &Path, import: &ModulePath) -> Option<Address> {
        self.layout().resolve(&self.project, from, import)
    }

    pub fn companions(&self, from: &Path, to: &Path) -> Vec<(PathBuf, PathBuf)> {
        self.layout().companions(&self.project, from, to)
    }

    /// `target` spelled from `from` in the style of `original`, as text.
    pub fn render(&self, from: &Path, target: &Address, original: &str) -> String {
        self.surgery()
            .render(&self.project, from, target, &self.syntax().parse(original))
            .to_string()
    }

    pub fn regroup(
        &self,
        from: &Path,
        source: &SourceText,
        entries: &[(ImportRef, Address)],
    ) -> Regrouped {
        self.surgery().regroup(&self.project, from, source, entries)
    }

    /// What the engine does for a move: ask the layout which files change,
    /// parse them, hand them to the surgery.
    pub fn relocate(&self, from: &Path, to: &Path) -> Result<Vec<SideEdit>, ResolveError> {
        self.relocate_widening(from, to, None)
    }

    /// `relocate` with the reach the engine found the moved `mod` line needs.
    pub fn relocate_widening(
        &self,
        from: &Path,
        to: &Path,
        widen_to: Option<ReachKind>,
    ) -> Result<Vec<SideEdit>, ResolveError> {
        let touched = self.layout().touched_by_move(&self.project, from, to)?;
        let parsed: Vec<(PathBuf, SourceText, Facts)> = touched
            .into_iter()
            .map(|path| {
                let text = self
                    .files
                    .get(&path)
                    .ok_or_else(|| ResolveError::Missing(path.clone().into()))?;
                let facts = self
                    .language
                    .facts(text)
                    .map_err(|e| ResolveError::Syntax {
                        path: path.clone().into(),
                        reason: e.to_string(),
                    })?;
                Ok((path, SourceText::new(text.as_str()), facts))
            })
            .collect::<Result<_, ResolveError>>()?;
        let views: Vec<Parsed<'_>> = parsed
            .iter()
            .map(|(path, source, facts)| Parsed {
                path,
                source,
                facts,
            })
            .collect();
        self.surgery()
            .relocate(&self.project, from, to, &views, widen_to)
    }
}
