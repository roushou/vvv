//! How files and import paths map onto a language's namespace.
//!
//! Two capabilities, both pure: a [`Layout`] knows how a project is arranged
//! — which [`Address`] a file has, what an import path written in a file
//! points at, where a module may live — and a [`Surgery`] knows how to spell
//! an edit in the language — a path from one file to an address, a grouped
//! import rewritten, the declarations a move drags along. Neither touches a
//! file system: both read a [`Project`], the workspace as data the engine
//! built from its walk, and return plain values.

mod address;
mod project;

use std::path::{Path, PathBuf};

pub use address::{Address, Dependency, Package, PackageId, Packages};
pub use project::{FileSet, Parsed, Project};

use crate::edit::Edit;
use crate::import::ImportRef;
use crate::paths::{ModulePath, RelPath};
use crate::semantics::ReachKind;
use crate::symbol::Symbol;
use crate::text::{SourceText, Span};

/// Why a resolver could not address, read, or relocate a file. Each variant
/// is a distinct situation with its own remedy; the message is one rendering.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ResolveError {
    #[error("{} is not addressable in this language (outside the module tree?)", .0.display())]
    NotAddressable(RelPath),
    #[error("{} does not exist", .0.display())]
    Missing(RelPath),
    #[error("{} is a crate or directory root; moving it is not supported", .0.display())]
    Root(RelPath),
    #[error("{} would end up inside itself at {}", from.display(), to.display())]
    IntoItself { from: RelPath, to: RelPath },
    #[error("moving between crates is not supported ({} → {})", from.display(), to.display())]
    CrossProject { from: RelPath, to: RelPath },
    #[error("cannot find the file declaring `{name}`")]
    NoDeclaringFile { name: String },
    #[error(
        "no file declares module `{module}`; create {} first so `{declaration}` has a home",
        Candidates(candidates)
    )]
    NoParentFile {
        module: String,
        candidates: Vec<RelPath>,
        declaration: String,
    },
    #[error("no `{declaration}` in {}; inline or #[path] modules are not supported", parent.display())]
    NoDeclaration {
        declaration: String,
        parent: RelPath,
    },
    #[error("cannot parse {}: {reason}", path.display())]
    Syntax { path: RelPath, reason: String },
}

/// `a.rs (or a/mod.rs)` for error messages.
struct Candidates<'a>(&'a [RelPath]);

impl std::fmt::Display for Candidates<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            [] => write!(f, "the module file"),
            [only] => write!(f, "{}", only.display()),
            [first, rest @ ..] => {
                write!(f, "{}", first.display())?;
                write!(f, " (or ")?;
                for (i, p) in rest.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p.display())?;
                }
                write!(f, ")")
            }
        }
    }
}

/// Extra change a move requires in a file other than import rewrites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideEdit {
    pub path: PathBuf,
    pub edit: Edit,
}

/// How a language's projects are arranged on disk: a pure function of the
/// [`Project`] — no file is read, no existence probed, beyond what the
/// project already lists.
pub trait Layout: Send + Sync {
    /// File names of manifests that declare a package (`Cargo.toml`).
    fn manifests(&self) -> &'static [&'static str];

    /// The package a manifest declares, from its text; `None` when it
    /// declares none (a workspace-only `Cargo.toml`).
    fn package(&self, manifest: &Path, text: &str) -> Option<Package>;

    /// Address a file or directory at `path` (workspace-relative) has or
    /// would have.
    fn address(&self, project: &Project, path: &Path) -> Result<Address, ResolveError>;

    /// Where a module at `address` may be declared, most conventional first
    /// (`a.rs` before `a/mod.rs`); the project says which exists.
    fn candidates(&self, project: &Project, address: &Address) -> Vec<PathBuf>;

    /// What `import` written in `from` points at, if the layout can tell. A
    /// path into a dependency outside the workspace resolves into that
    /// package's address space (its files are unknown, its name is not);
    /// unresolvable relative paths and unknown heads yield `None`.
    fn resolve(&self, project: &Project, from: &Path, import: &ModulePath) -> Option<Address>;

    /// Paths that must move together with `from → to` because the language
    /// treats them as one unit: Rust's `a.rs` and `a/`. Each pair is
    /// (current path, destination); directories are expanded by the caller.
    fn companions(&self, project: &Project, from: &Path, to: &Path) -> Vec<(PathBuf, PathBuf)> {
        let _ = (project, from, to);
        Vec::new()
    }

    /// Files whose contents a move of `from` to `to` changes beyond import
    /// paths — the old and new parent of a Rust module — or why the move is
    /// impossible. The engine parses them and hands them to
    /// [`Surgery::relocate`].
    fn touched_by_move(
        &self,
        project: &Project,
        from: &Path,
        to: &Path,
    ) -> Result<Vec<PathBuf>, ResolveError> {
        let _ = (project, from, to);
        Ok(Vec::new())
    }
}

/// How to spell an edit in a language. Text and facts in, edits out; never
/// a file.
pub trait Surgery: Send + Sync {
    /// Spell a reference to `target` as `from` should write it. `original` is
    /// the text being replaced, so the style (relative vs absolute, with or
    /// without extension) can be kept.
    fn render(
        &self,
        project: &Project,
        from: &Path,
        target: &Address,
        original: &ModulePath,
    ) -> ModulePath;

    /// The statement that brings `name`, declared at `target`, into `from`:
    /// `use crate::a::Name;`, `import { Name } from './a';`. `None` when the
    /// language has no import statement vvv can write.
    fn import_statement(
        &self,
        project: &Project,
        from: &Path,
        target: &Address,
        name: &str,
    ) -> Option<String> {
        let _ = (project, from, target, name);
        None
    }

    /// Rewrite grouped entries of one statement so each points at its new
    /// target: in place when the target stays under the group's prefix,
    /// otherwise by moving the entry out into its own statement. `from` is
    /// where the statement will live; `entries` all share one
    /// [`ImportGroup::statement`](crate::import::ImportGroup::statement). Every requested entry must have exactly one explicit outcome;
    /// skipped entries become notices. Prefix addresses are supplied by the
    /// caller after its planned prefix and binding changes, never re-resolved.
    fn regroup(
        &self,
        project: &Project,
        from: &Path,
        source: &SourceText,
        imports: &GroupedImports,
    ) -> Regrouped {
        let _ = (project, from, source);
        Regrouped::skipped(imports)
    }

    /// Changes beyond import rewrites when `from` becomes `to`, computed from
    /// the parsed files the layout named in [`Layout::touched_by_move`].
    /// `widen_to` is the reach the moved declaration (Rust's `mod` line)
    /// needs at its new parent, when the engine found consumers its current
    /// modifier would no longer admit; `None` keeps it as written.
    fn relocate(
        &self,
        project: &Project,
        from: &Path,
        to: &Path,
        touched: &[Parsed<'_>],
        widen_to: Option<ReachKind>,
    ) -> Result<Vec<SideEdit>, ResolveError> {
        let _ = (project, from, to, touched, widen_to);
        Ok(Vec::new())
    }

    /// An import statement for a path as written (`use std::fmt;`), for
    /// imports the layout cannot resolve but a moved item still needs.
    fn import_of_path(&self, path: &ModulePath, name: &str) -> Option<String> {
        let _ = (path, name);
        None
    }

    /// Where a new import statement goes in a file: after its last import,
    /// else after what must stay first (Rust's `//!` docs and `#![…]`).
    fn import_insertion(&self, file: &Parsed<'_>) -> usize {
        file.facts
            .imports
            .iter()
            .filter(|i| i.declares)
            .map(|i| i.span.end)
            .max()
            .map_or(0, |end| {
                file.source.as_str()[end..]
                    .find('\n')
                    .map_or(file.source.len(), |nl| end + nl + 1)
            })
    }

    /// Where a new declaration goes in a file: the end.
    fn item_insertion(&self, file: &Parsed<'_>) -> usize {
        file.source.len()
    }

    /// The edit that gives `symbol` the reach `to`: its modifier replaced, or
    /// one inserted before the declaration. `None` when the language cannot
    /// say it, or will not — `Everyone` is never spelled by vvv.
    fn widen(&self, symbol: &Symbol, source: &SourceText, to: ReachKind) -> Option<Edit> {
        let _ = (symbol, source, to);
        None
    }
}

/// One grouped entry's required target and its prefix's meaning after the move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedImport {
    pub import: ImportRef,
    pub target: Address,
    pub prefix: Option<Address>,
}

/// Unique grouped entries from one statement, validated at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedImports {
    entries: Vec<GroupedImport>,
    statement: Span,
}

impl GroupedImports {
    pub fn new(entries: Vec<GroupedImport>) -> Result<Self, RegroupError> {
        let first = entries.first().ok_or(RegroupError::Empty)?;
        let statement = first
            .import
            .group
            .as_ref()
            .ok_or(RegroupError::NotGrouped(first.import.span))?
            .statement;
        for (i, entry) in entries.iter().enumerate() {
            let group = entry
                .import
                .group
                .as_ref()
                .ok_or(RegroupError::NotGrouped(entry.import.span))?;
            if entry.import.path.head != group.prefix.head
                || !entry
                    .import
                    .path
                    .segments
                    .starts_with(&group.prefix.segments)
            {
                return Err(RegroupError::InvalidPrefix(entry.import.span));
            }
            if group.statement != statement {
                return Err(RegroupError::DifferentStatement(entry.import.span));
            }
            if entries[..i]
                .iter()
                .any(|e| e.import.span == entry.import.span)
            {
                return Err(RegroupError::DuplicateEntry(entry.import.span));
            }
        }
        Ok(Self { entries, statement })
    }

    pub fn entries(&self) -> &[GroupedImport] {
        &self.entries
    }
    pub fn statement(&self) -> Span {
        self.statement
    }
}

/// The actual handling of one requested entry. In-place replacement is the
/// text for the entry span, not a separately rendered full path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegroupedOutcome {
    InPlace { replacement: ModulePath },
    Structural,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegroupedEntry {
    pub span: Span,
    pub outcome: RegroupedOutcome,
}

/// Outcome of [`Surgery::regroup`], checked against its request before use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Regrouped {
    pub edits: Vec<Edit>,
    pub outcomes: Vec<RegroupedEntry>,
}

impl Regrouped {
    pub fn skipped(imports: &GroupedImports) -> Self {
        Self {
            edits: Vec::new(),
            outcomes: imports
                .entries()
                .iter()
                .map(|e| RegroupedEntry {
                    span: e.import.span,
                    outcome: RegroupedOutcome::Skipped,
                })
                .collect(),
        }
    }

    pub fn validate<'a>(
        &'a self,
        imports: &'a GroupedImports,
    ) -> Result<Vec<(&'a GroupedImport, &'a RegroupedOutcome)>, RegroupError> {
        for (i, outcome) in self.outcomes.iter().enumerate() {
            if !imports
                .entries()
                .iter()
                .any(|e| e.import.span == outcome.span)
            {
                return Err(RegroupError::ForeignOutcome(outcome.span));
            }
            if self.outcomes[..i].iter().any(|o| o.span == outcome.span) {
                return Err(RegroupError::DuplicateOutcome(outcome.span));
            }
        }
        imports
            .entries()
            .iter()
            .map(|entry| {
                let outcome = self
                    .outcomes
                    .iter()
                    .find(|o| o.span == entry.import.span)
                    .ok_or(RegroupError::MissingOutcome(entry.import.span))?;
                if let RegroupedOutcome::InPlace { replacement } = &outcome.outcome {
                    let matching: Vec<_> = self
                        .edits
                        .iter()
                        .filter(|e| e.span == entry.import.span)
                        .collect();
                    if matching.len() != 1 || matching[0].replacement != replacement.to_string() {
                        return Err(RegroupError::InconsistentReplacement(entry.import.span));
                    }
                }
                Ok((entry, &outcome.outcome))
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegroupError {
    #[error("a grouped rewrite requires at least one entry")]
    Empty,
    #[error("entry {0:?} is not grouped")]
    NotGrouped(Span),
    #[error("entry {0:?} belongs to another statement")]
    DifferentStatement(Span),
    #[error("entry {0:?} does not extend its group prefix")]
    InvalidPrefix(Span),
    #[error("duplicate grouped entry {0:?}")]
    DuplicateEntry(Span),
    #[error("grouped rewrite returned a foreign entry {0:?}")]
    ForeignOutcome(Span),
    #[error("grouped rewrite returned entry {0:?} twice")]
    DuplicateOutcome(Span),
    #[error("grouped rewrite omitted entry {0:?}")]
    MissingOutcome(Span),
    #[error("grouped rewrite replacement for {0:?} differs from its edit")]
    InconsistentReplacement(Span),
}

#[cfg(test)]
mod regroup_tests {
    use super::*;
    use crate::{ImportGroup, PathSyntax};

    struct Fixture {
        imports: GroupedImports,
    }

    impl Fixture {
        fn new() -> Self {
            let prefix = PathSyntax::Scoped.parse("crate::a");
            let mut import =
                ImportRef::new(Span::new(15, 16), PathSyntax::Scoped.parse("crate::a::X"));
            import.group = Some(ImportGroup {
                prefix,
                item: import.span,
                list: Span::new(14, 17),
                items: 1,
                statement: Span::new(0, 18),
                top_level: true,
            });
            Self {
                imports: GroupedImports::new(vec![GroupedImport {
                    import,
                    target: Address::new("p", ["b", "X"]),
                    prefix: Some(Address::new("p", ["a"])),
                }])
                .unwrap(),
            }
        }

        fn span(&self) -> Span {
            self.imports.entries()[0].import.span
        }
        fn outcome(&self, outcome: RegroupedOutcome) -> Regrouped {
            Regrouped {
                edits: vec![],
                outcomes: vec![RegroupedEntry {
                    span: self.span(),
                    outcome,
                }],
            }
        }
    }

    #[test]
    fn grouped_requests_require_unique_entries_from_one_statement() {
        let fixture = Fixture::new();
        assert_eq!(GroupedImports::new(vec![]), Err(RegroupError::Empty));
        let entry = fixture.imports.entries()[0].clone();
        assert_eq!(
            GroupedImports::new(vec![entry.clone(), entry.clone()]),
            Err(RegroupError::DuplicateEntry(entry.import.span))
        );
        let mut other = entry.clone();
        other.import.group.as_mut().unwrap().statement = Span::new(20, 40);
        assert_eq!(
            GroupedImports::new(vec![entry.clone(), other]),
            Err(RegroupError::DifferentStatement(entry.import.span))
        );
        let mut ungrouped = entry;
        ungrouped.import.group = None;
        assert_eq!(
            GroupedImports::new(vec![ungrouped]),
            Err(RegroupError::NotGrouped(fixture.span()))
        );
    }

    #[test]
    fn regrouped_outcomes_cover_exactly_the_requested_entries() {
        let fixture = Fixture::new();
        let mut out = fixture.outcome(RegroupedOutcome::Structural);
        assert!(out.validate(&fixture.imports).is_ok());
        out.outcomes.push(out.outcomes[0].clone());
        assert_eq!(
            out.validate(&fixture.imports),
            Err(RegroupError::DuplicateOutcome(fixture.span()))
        );
        out.outcomes.pop();
        out.outcomes[0].span = Span::new(30, 31);
        assert_eq!(
            out.validate(&fixture.imports),
            Err(RegroupError::ForeignOutcome(Span::new(30, 31)))
        );
        out.outcomes.clear();
        assert_eq!(
            out.validate(&fixture.imports),
            Err(RegroupError::MissingOutcome(fixture.span()))
        );
    }

    #[test]
    fn in_place_outcomes_report_the_actual_replacement() {
        let fixture = Fixture::new();
        let mut out = fixture.outcome(RegroupedOutcome::InPlace {
            replacement: PathSyntax::Scoped.parse("Y"),
        });
        assert_eq!(
            out.validate(&fixture.imports),
            Err(RegroupError::InconsistentReplacement(fixture.span()))
        );
        out.edits.push(Edit::replace(fixture.span(), "different"));
        assert_eq!(
            out.validate(&fixture.imports),
            Err(RegroupError::InconsistentReplacement(fixture.span()))
        );
        out.edits[0].replacement = "Y".into();
        assert!(out.validate(&fixture.imports).is_ok());
    }

    #[test]
    fn default_grouped_outcomes_explicitly_skip_every_target() {
        let fixture = Fixture::new();
        let out = Regrouped::skipped(&fixture.imports);
        let validated = out.validate(&fixture.imports).unwrap();
        assert_eq!(validated.len(), 1);
        assert_eq!(validated[0].0.target, Address::new("p", ["b", "X"]));
        assert!(matches!(validated[0].1, RegroupedOutcome::Skipped));
        assert!(out.edits.is_empty());
    }
}
