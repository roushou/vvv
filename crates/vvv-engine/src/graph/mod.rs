//! What vvv knows about a tree, and the questions commands ask of it.
//!
//! The store: every file, which language claims it, its text, and — once
//! asked — its facts, each kept as long as the file's [`Stamp`] stays the
//! same; per language, the [`Project`] its layout resolves paths against.
//! One walk serves everything a command needs. Kept for a session, the
//! graph re-reads only files that changed and parses a file once however
//! many questions are asked of it; built per call, it is the workspace read
//! once.
//!
//! The questions: what a search finds, what declares a name, what a file imports and who imports a module. Each
//! is answered here, through a [`Namespace`], so a command composes answers
//! instead of fetching languages, layouts and projects itself.

mod candidate;
mod fragment;
mod namespace;
mod references;
mod scope;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use vvv_core::{
    Address, FileSet, Language, LanguageId, LanguageRegistry, Name, Oracle, Package, Packages,
    Project, Query, Role, SourceText,
};

pub use candidate::Candidate;
pub use fragment::{Declared, Edge, Fragment, Node, Structure};
pub use namespace::Namespace;
use references::{Declaration, Target};
pub use references::{DeclarationSite, Evidence};
pub use scope::Scope;

use crate::{
    Dep, EngineError, Importer, Match, Occurrence, Reach, Reason, ReferencesQuery, Search, Skipped,
    Stamp, Workspace,
};

pub struct Graph {
    workspace: Workspace,
    languages: LanguageRegistry,
    /// Every file of the last walk, absolute, in path order. Only a project
    /// needs them relative and as a set, so that is built on demand.
    walked: Vec<PathBuf>,
    /// When that walk happened; `None` once something may have changed it.
    walked_at: Option<Instant>,
    /// Every claimed file, in path order.
    entries: Vec<Entry>,
    /// The workspace as a language's layout sees it, built from the last
    /// walk the first time that language asks; a search never pays for it.
    /// A project equal to the previous build keeps its allocation, so
    /// fragments resolved against it stay valid.
    projects: HashMap<LanguageId, Arc<Project>>,
    /// A second opinion for tokens syntax cannot place, when the host has
    /// one; asked after the scope, never instead of it.
    oracle: Option<Arc<dyn Oracle>>,
    /// The last build of each language's project, to tell a refresh that
    /// changed nothing about the project from one that did.
    previous: HashMap<LanguageId, Arc<Project>>,
}

struct Entry {
    /// Absolute, as walked.
    path: PathBuf,
    /// `None` when the model will not be asked again, so nothing was stamped.
    stamp: Option<Stamp>,
    candidate: Candidate,
}

/// How long an [`Engine`](crate::Engine) keeps its graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Retention {
    /// Read the workspace afresh for every question: right for a one-shot
    /// command, whose single question has to read the tree anyway.
    #[default]
    PerCall,
    /// Keep files and facts between questions and re-read only what changed:
    /// right for a session that asks the same tree over and over. Within
    /// `trust` of the last walk it does not even look — a burst of
    /// keystrokes walks a large tree once, not per key — except after the
    /// engine itself wrote, or was told the tree changed.
    Session { trust: Duration },
}

impl Retention {
    /// A session that checks the tree before every question.
    pub const fn session() -> Self {
        Self::Session {
            trust: Duration::ZERO,
        }
    }

    /// A session that trusts its last walk for `trust`.
    pub const fn trusting(self, trust: Duration) -> Self {
        match self {
            Self::PerCall => Self::PerCall,
            Self::Session { .. } => Self::Session { trust },
        }
    }
}

impl Graph {
    pub fn new(workspace: Workspace, languages: LanguageRegistry) -> Self {
        Self {
            workspace,
            languages,
            walked: Vec::new(),
            walked_at: None,
            oracle: None,
            entries: Vec::new(),
            projects: HashMap::new(),
            previous: HashMap::new(),
        }
    }

    /// Walk the tree; load files that appeared or changed, forget files that
    /// vanished, keep the rest. A graph kept for a session stamps what it
    /// loads so the next refresh can tell; one built per call does not.
    pub fn refresh(&mut self, retention: Retention) -> Result<(), EngineError> {
        if let Retention::Session { trust } = retention
            && self.walked_at.is_some_and(|at| at.elapsed() < trust)
        {
            return Ok(());
        }
        let walked = self.workspace.files()?;
        let claimed: Vec<(&Path, Arc<dyn Language>)> = walked
            .iter()
            .filter_map(|abs| Some((abs.as_path(), self.languages.for_path(abs)?)))
            .collect();

        let before = std::mem::take(&mut self.entries);
        let vfs = self.workspace.vfs();
        let workspace = &self.workspace;
        let stamping = matches!(retention, Retention::Session { .. });
        let now: Vec<Entry> = claimed
            .into_par_iter()
            .map(|(abs, language)| {
                let stamp = stamping.then(|| vfs.stamp(abs)).transpose()?;
                let known = stamp.and_then(|stamp| {
                    before
                        .binary_search_by(|e| e.path.as_path().cmp(abs))
                        .ok()
                        .map(|i| &before[i])
                        .filter(|e| e.stamp == Some(stamp))
                });
                let candidate = match known {
                    Some(known) => known.candidate.clone(),
                    None => Candidate::new(workspace.load(abs)?, language),
                };
                Ok(Entry {
                    path: abs.to_path_buf(),
                    stamp,
                    candidate,
                })
            })
            .collect::<Result<_, EngineError>>()?;
        self.entries = now;
        self.previous.extend(std::mem::take(&mut self.projects));
        self.walked = walked;
        self.walked_at = Some(Instant::now());
        Ok(())
    }

    /// The tree may have changed — the engine wrote to it, or was told so:
    /// the next refresh walks whatever it was told to trust.
    pub fn touched(&mut self) {
        self.walked_at = None;
    }

    /// Consult `oracle` for the tokens the scope leaves unresolved.
    pub fn with_oracle(mut self, oracle: Arc<dyn Oracle>) -> Self {
        self.oracle = Some(oracle);
        self
    }

    pub(crate) fn oracle(&self) -> Option<&Arc<dyn Oracle>> {
        self.oracle.as_ref()
    }

    /// Every file `language` claims — any registered language when `None` —
    /// in path order.
    pub fn files(&self, language: Option<&LanguageId>) -> Vec<Candidate> {
        self.entries
            .iter()
            .map(|e| &e.candidate)
            .filter(|c| language.is_none_or(|id| &c.language() == id))
            .cloned()
            .collect()
    }

    /// The one claimed file at `path` (absolute or workspace-relative).
    pub fn candidate(&self, path: &Path) -> Option<Candidate> {
        let abs = self.workspace.absolute(path);
        self.entries
            .binary_search_by(|e| e.path.as_path().cmp(abs.as_path()))
            .ok()
            .map(|i| self.entries[i].candidate.clone())
    }

    /// The files of `language` that spell every one of `literals` as whole
    /// tokens: the only ones a query built from those words can match.
    pub fn containing(&self, language: Option<&LanguageId>, literals: &[&str]) -> Vec<Candidate> {
        self.files(language)
            .into_par_iter()
            .filter(|c| c.contains_all(literals))
            .collect()
    }

    /// The workspace as `language`'s layout sees it: the walk's files plus
    /// the packages the layout's manifests declare. Built on first use after
    /// a refresh; `None` when the language has no layout.
    fn project_build(&mut self, language: &LanguageId) -> Option<Arc<Project>> {
        if let Some(built) = self.projects.get(language) {
            return Some(built.clone());
        }
        let language_impl = self.languages.get(language)?;
        let layout = language_impl.layout()?;
        let vfs = self.workspace.vfs();
        let files: FileSet = self
            .walked
            .iter()
            .map(|abs| self.workspace.absolute(abs))
            .map(|abs| self.workspace.relative(&abs))
            .collect();
        let manifests: Vec<&Path> = files
            .iter()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| layout.manifests().iter().any(|m| n == *m))
            })
            .collect();
        let packages: Vec<Package> = manifests
            .into_par_iter()
            .filter_map(|manifest| {
                let text = vfs.read(&self.workspace.absolute(manifest)).ok()?;
                layout.package(manifest, &text)
            })
            .collect();
        let packages = Packages::new(packages);
        let project = Project { packages, files };
        // The same project as last time keeps its allocation: every fragment
        // resolved against it is still right.
        let built = match self.previous.remove(language) {
            Some(last) if *last == project => last,
            _ => Arc::new(project),
        };
        self.projects.insert(language.clone(), built.clone());
        Some(built)
    }

    // ------------------------------------------------------------ questions

    /// Every language this graph understands.
    pub fn language_ids(&self) -> Vec<LanguageId> {
        self.languages.iter().map(|l| l.id()).collect()
    }

    /// The language claiming `path`, when one does.
    pub fn language_of(&self, path: &Path) -> Option<Arc<dyn Language>> {
        self.languages.for_path(path)
    }

    pub fn language(&self, id: &LanguageId) -> Option<Arc<dyn Language>> {
        self.languages.get(id)
    }

    /// The language of a directory: the one claiming its first file.
    pub fn language_of_directory(&self, dir: &Path) -> Option<Arc<dyn Language>> {
        self.workspace
            .vfs()
            .walk(&self.workspace.absolute(dir))
            .ok()?
            .iter()
            .find_map(|p| self.languages.for_path(p))
    }

    /// A language's module system on this tree, when it has a layout.
    pub fn namespace(&mut self, language: &LanguageId) -> Option<Namespace> {
        let language = self.languages.get(language)?;
        language.layout()?;
        let project = self.project_build(&language.id())?;
        Some(Namespace::new(language, project))
    }

    /// The namespace of the language claiming `path`, or why there is none:
    /// no language claims it, or the language has no layout.
    pub fn namespace_of(&mut self, path: &Path) -> Result<Namespace, EngineError> {
        let language = self
            .languages
            .for_path(path)
            .ok_or_else(|| EngineError::NoLanguage(path.into()))?;
        self.namespace(&language.id())
            .ok_or_else(|| EngineError::NoLayout(language.id()))
    }

    /// The claimed file at `path`, or the error that says why there is none.
    pub fn file(&self, path: &Path) -> Result<Candidate, EngineError> {
        self.candidate(path)
            .ok_or_else(|| EngineError::NoLanguage(path.into()))
    }

    /// Structural or symbolic search: only files spelling the query's
    /// literal words are parsed. A language whose grammar cannot compile the
    /// query is skipped and named, never fatal.
    ///
    /// Declarations come first, with their module address when the file has
    /// one, then everything else by path and position — the order human
    /// output prints and numbers.
    pub fn search(&mut self, query: &Query) -> Result<Search, EngineError> {
        let mut skipped = Vec::new();
        let mut accepted: Vec<LanguageId> = Vec::new();
        for language in self.languages.iter() {
            if query.language().is_some_and(|id| id != &language.id()) {
                continue;
            }
            match language.accepts(query) {
                Ok(()) => accepted.push(language.id()),
                Err(reason) => skipped.push(Skipped {
                    language: language.id(),
                    reason: reason.to_string(),
                }),
            }
        }
        let candidates: Vec<Candidate> = self
            .containing(query.language(), &query.literals())
            .into_iter()
            .filter(|c| accepted.contains(&c.language()))
            .collect();
        let per_file: Vec<Vec<Match>> = candidates
            .par_iter()
            .map(|c| c.find(query))
            .collect::<Result<_, _>>()?;
        // Candidates come in path order and each file's matches in position
        // order, so moving declarations to the front is all the ordering
        // there is to do.
        let mut matches: Vec<Match> = per_file.into_iter().flatten().collect();
        self.address_declarations(&mut matches);
        matches.sort_by_key(|m| m.role != Role::Declaration);
        Ok(Search {
            query: query.clone(),
            matches,
            skipped,
        })
    }

    /// Give each declaration the address `outline` would: its file's module
    /// joined with its name, when the language has a layout, the file is
    /// addressable and the kind is. One address lookup per file, relying on
    /// a file's matches being contiguous.
    fn address_declarations(&mut self, matches: &mut [Match]) {
        let mut last: Option<(PathBuf, Option<Address>)> = None;
        for m in matches.iter_mut() {
            let Some(symbol) = &m.symbol else {
                continue;
            };
            let Some(ns) = self.namespace(&m.language) else {
                continue;
            };
            if last.as_ref().is_none_or(|(path, _)| path != &m.path) {
                last = Some((m.path.to_path_buf(), ns.address(&m.path).ok()));
            }
            m.address = last
                .as_ref()
                .and_then(|(_, module)| module.as_ref())
                .and_then(|module| ns.address_of(module, symbol));
        }
    }

    /// Declarations called `query.name`, narrowed by kind, language and
    /// `declared_in`; an error when there are none.
    pub fn declarations(&mut self, query: &ReferencesQuery) -> Result<Vec<Match>, EngineError> {
        let mut search = Query::named(query.name.as_str());
        if let Some(symbol) = query.symbol {
            search = search.with_symbol(symbol);
        }
        if let Some(language) = &query.language {
            search = search.in_language(language.clone());
        }
        let mut declarations = self.search(&search)?.matches;
        if let Some(file) = &query.declared_in {
            let file = self.workspace.normalize(file);
            declarations.retain(|d| d.path == file);
        }
        if declarations.is_empty() {
            return Err(EngineError::NoSuchSymbol {
                name: query.name.clone(),
                kind: query.symbol,
            });
        }
        Ok(declarations)
    }

    /// What a file's import statements name, each resolved to an address,
    /// followed through re-exports to the declaration it reaches, and the
    /// file declaring that when the layout can place it.
    pub fn imports_of(&mut self, path: &Path) -> Result<Vec<Dep>, EngineError> {
        let candidate = self.file(path)?;
        let ns = self.namespace_of(path)?;
        let source = candidate.file().source();
        let fragment = candidate.fragment(&ns)?;
        fragment
            .imports()
            .map(|edge| self.dep(&ns, source, edge))
            .collect()
    }

    /// One import edge as an answer names it.
    pub fn dep(
        &self,
        ns: &Namespace,
        source: &SourceText,
        edge: &Edge,
    ) -> Result<Dep, EngineError> {
        let origin = match edge.address() {
            Some(address) => self.origin_of(ns, address)?.filter(|o| o != address),
            None => None,
        };
        let file = origin
            .as_ref()
            .or(edge.address())
            .and_then(|a| ns.file_of(a));
        Ok(Dep {
            import: edge.import.clone(),
            start: source.position(edge.import.span.start),
            address: edge.address().cloned(),
            origin,
            file: file.map(Into::into),
        })
    }

    /// Every file of `ns`'s language with its edges, in parallel, path order.
    pub fn fragments(&self, ns: &Namespace) -> Result<Vec<Node>, EngineError> {
        self.files(Some(&ns.id()))
            .into_par_iter()
            .map(|candidate| {
                Ok(Node {
                    fragment: candidate.fragment(ns)?,
                    candidate,
                })
            })
            .collect()
    }

    /// Every file of `ns`'s language other than `path` with a declared
    /// import resolving to an address `leads` accepts, in path order.
    pub fn importers(
        &self,
        ns: &Namespace,
        path: &Path,
        leads: impl Fn(&Address) -> bool + Sync,
    ) -> Result<Vec<Importer>, EngineError> {
        Ok(self
            .fragments(ns)?
            .into_iter()
            .filter(|node| node.path() != path)
            .flat_map(|node| {
                node.fragment
                    .imports()
                    .filter(|e| e.address().is_some_and(&leads))
                    .map(|e| Importer {
                        path: node.path().into(),
                        import: e.import.clone(),
                        start: node.candidate.file().source().position(e.import.span.start),
                    })
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    /// Gather what a rename rests on. Occurrences come only from the
    /// languages in which a matching declaration exists, so a Rust `foo`
    /// never touches a TypeScript `foo`.
    pub fn references(&mut self, query: &ReferencesQuery) -> Result<Evidence, EngineError> {
        let graph = self;
        let declarations = graph.declarations(query)?;
        let languages: BTreeSet<LanguageId> =
            declarations.iter().map(|m| m.language.clone()).collect();
        let mut occurrences = Vec::new();
        // Languages whose name has more than one declaration: an unresolved
        // token there could belong to any of them.
        let mut ambiguous = BTreeSet::new();
        for language in languages {
            let semantics = graph
                .language(&language)
                .map(|l| l.semantics())
                .ok_or_else(|| EngineError::NoLayout(language.clone()))?;
            let addressable = Declaration::addressable_in(&declarations, &language, semantics);
            let mut target = Target::of(graph, query, &addressable)?;
            if let Some(target) = &mut target
                && let Some(ns) = graph.namespace(&language)
            {
                target.aliases = graph.aliases_of(&ns, &target.address, &query.name)?;
            }
            if addressable.len() > 1 || target.as_ref().is_some_and(|t| !t.others.is_empty()) {
                ambiguous.insert(language.clone());
            }
            // No token can spell a name the text does not contain.
            let candidates = graph.containing(Some(&language), &[&query.name]);
            let per_file: Vec<Vec<Occurrence>> = candidates
                .par_iter()
                .map(|c| match &target {
                    Some(target) => target.judge(c, &query.name),
                    // Nothing to judge against: every token is a candidate.
                    None => Ok(c
                        .references(&query.name)?
                        .into_iter()
                        .map(|m| Occurrence::judged(m, Reason::ByName))
                        .collect()),
                })
                .collect::<Result<_, _>>()?;
            let mut judged: Vec<Occurrence> = per_file.into_iter().flatten().collect();
            if let (Some(oracle), Some(target)) = (graph.oracle(), &target) {
                graph.consult(oracle.as_ref(), target, &mut judged)?;
            }
            occurrences.extend(judged);
        }
        // Grouped as a preview lists them — `✓`, then `?`, then `✗`, and
        // within those by reason — so the rows to add or drop with
        // `--select` are one range.
        occurrences.sort_by_key(|o| o.reason);
        Ok(Evidence {
            declarations,
            occurrences,
            ambiguous,
        })
    }

    /// Every address that reaches `target` through re-exports, `target`
    /// itself first: a `pub use` of the target (or of an alias of it) in a
    /// file offers it on under that file's address — as the name it binds,
    /// so `pub use a::X as Y` offers `Y`; a glob re-export of an alias's
    /// module offers it under its own name. Followed to a fixed point, so
    /// chains of re-exports resolve. Only files spelling one of the names
    /// found so far (or a `*`) are read.
    pub fn aliases_of(
        &self,
        ns: &Namespace,
        target: &Address,
        name: &str,
    ) -> Result<Vec<Address>, EngineError> {
        let mut aliases = vec![target.clone()];
        let mut names: BTreeSet<Name> = [Name::from(name)].into();
        let mut read: HashSet<PathBuf> = HashSet::new();
        let mut files: Vec<Arc<Fragment>> = Vec::new();
        // A named re-export spells the name. A glob re-export spells the
        // language's glob marker, and sits in the alias's own package or
        // spells that package's name in its path (`pub use foo_core::*`).
        let glob = ns.language().glob_marker();
        let reach = self.reach_of(ns, target)?;
        loop {
            let packages: BTreeSet<vvv_core::PackageId> =
                aliases.iter().map(|a| a.package().clone()).collect();
            let globs_toward = |c: &Candidate| {
                glob.is_some_and(|marker| c.text().contains(marker))
                    && (packages.iter().any(|p| c.contains_all(&[p.as_str()]))
                        || ns
                            .address(c.path())
                            .is_ok_and(|a| packages.contains(a.package())))
            };
            let fresh: Vec<(PathBuf, Arc<Fragment>)> = self
                .files(Some(&ns.id()))
                .into_par_iter()
                .filter(|c| !read.contains(c.path()))
                .filter(|c| names.iter().any(|n| c.contains_all(&[n.as_str()])) || globs_toward(c))
                .map(|c| Ok((c.path().to_path_buf(), c.fragment(ns)?)))
                .collect::<Result<_, EngineError>>()?;
            for (path, fragment) in fresh {
                read.insert(path);
                files.push(fragment);
            }
            let mut grew = false;
            for fragment in &files {
                let Some(module) = &fragment.module else {
                    continue;
                };
                for edge in fragment.edges.iter().filter(|e| e.import.reexport) {
                    let Some(resolved) = edge.address() else {
                        continue;
                    };
                    let offered: Vec<Name> = if edge.import.glob {
                        // A glob brings in what the module can see: the
                        // declaration itself only where its reach admits.
                        aliases
                            .iter()
                            .filter(|a| a.parent().as_ref() == Some(resolved))
                            .filter(|a| *a != target || reach.admits(module))
                            .filter_map(|a| a.path().last().cloned())
                            .collect()
                    } else if aliases.contains(resolved) {
                        edge.import.binding().cloned().into_iter().collect()
                    } else {
                        Vec::new()
                    };
                    for name in offered {
                        let alias = module.join(name.as_str());
                        if !aliases.contains(&alias) {
                            names.insert(name);
                            aliases.push(alias);
                            grew = true;
                        }
                    }
                }
            }
            if !grew {
                return Ok(aliases);
            }
        }
    }

    /// Ask the oracle about every token the scope left unresolved. An
    /// answer counts only as a declaration the graph knows: the target's
    /// own name is `Oracle`, another addressable declaration's is
    /// `OracleOther`, anything else leaves the token as it was.
    fn consult(
        &self,
        oracle: &dyn Oracle,
        target: &Target,
        occurrences: &mut [Occurrence],
    ) -> Result<(), EngineError> {
        for occurrence in occurrences
            .iter_mut()
            .filter(|o| o.reason == Reason::Unresolved)
        {
            let Some(referent) = oracle.refers(&occurrence.m.path, occurrence.m.span) else {
                continue;
            };
            let Some(candidate) = self.candidate(&referent.path) else {
                continue;
            };
            let fragment = candidate.fragment(&target.ns)?;
            let Some(declared) = fragment
                .declarations
                .iter()
                .find(|d| d.symbol.name_span == referent.name_span)
            else {
                continue;
            };
            let reason = if target.aliases.contains(&declared.address) {
                Reason::Oracle
            } else {
                Reason::OracleOther
            };
            *occurrence = Occurrence::judged(occurrence.m.clone(), reason);
        }
        Ok(())
    }

    /// Who may name the declaration at `address`, as its file declares it;
    /// everyone when the graph cannot find it (a re-export's own reach).
    fn reach_of(&self, ns: &Namespace, address: &Address) -> Result<Reach, EngineError> {
        let Some(file) = address.parent().and_then(|module| ns.file_of(&module)) else {
            return Ok(Reach::Everyone);
        };
        let fragment = self.file(&file)?.fragment(ns)?;
        Ok(fragment
            .declarations
            .iter()
            .find(|d| d.address == *address)
            .map_or(Reach::Everyone, |d| d.reach.clone()))
    }

    /// The declaration an address reaches through re-exports: the address
    /// itself when its module declares the name; else what the module's
    /// re-export of the name (a `pub use` binding it, or a glob of a module
    /// that has it) leads to, followed to the end. `None` when the chain
    /// leaves what the layout can place — another package, a type's item.
    pub fn origin_of(
        &self,
        ns: &Namespace,
        address: &Address,
    ) -> Result<Option<Address>, EngineError> {
        self.origin_from(ns, address, &mut HashSet::new())
    }

    fn origin_from(
        &self,
        ns: &Namespace,
        at: &Address,
        seen: &mut HashSet<Address>,
    ) -> Result<Option<Address>, EngineError> {
        if !seen.insert(at.clone()) {
            return Ok(None);
        }
        let (Some(module), Some(name)) = (at.parent(), at.path().last()) else {
            // A package root names itself.
            return Ok(Some(at.clone()));
        };
        let Some(file) = ns.file_of(&module) else {
            return Ok(None);
        };
        let fragment = self.file(&file)?.fragment(ns)?;
        if fragment.module.as_ref() != Some(&module) {
            return Ok(None);
        }
        if fragment.declarations.iter().any(|d| *name == d.symbol.name) {
            return Ok(Some(at.clone()));
        }
        for edge in fragment.edges.iter().filter(|e| e.import.reexport) {
            let Some(resolved) = edge.address() else {
                continue;
            };
            let next = if edge.import.glob {
                resolved.join(name.as_str())
            } else if edge.import.binding() == Some(name) {
                resolved.clone()
            } else {
                continue;
            };
            if let Some(origin) = self.origin_from(ns, &next, seen)? {
                return Ok(Some(origin));
            }
        }
        Ok(None)
    }

    /// Modules with an import or a qualified path resolving to `address` or
    /// under it — every file of the namespace's language except `except`,
    /// in path order. What must still see a declaration after it moves.
    pub fn consumers(
        &self,
        ns: &Namespace,
        address: &Address,
        except: &[&Path],
    ) -> Result<Vec<Address>, EngineError> {
        Ok(self
            .fragments(ns)?
            .into_iter()
            .filter(|node| !except.contains(&node.path()))
            .filter(|node| node.fragment.reaches(address))
            .filter_map(|node| node.fragment.module.clone())
            .collect())
    }
}

impl std::fmt::Debug for Graph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graph")
            .field("root", &self.workspace.root())
            .field("files", &self.walked.len())
            .field("loaded", &self.entries.len())
            .finish()
    }
}
