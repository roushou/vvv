//! The reference edges of the graph, derived per name: the declarations
//! called `name`, the [`Target`] among them that is meant, and every token
//! spelling the name in the declaring language(s), each judged against the
//! target through its file's [`Scope`]. What `rename`, `references` and a
//! symbol move rest on.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;
use vvv_core::RelPath;
use vvv_core::{Address, LanguageId, Name, Oracle, Position, Query};

use super::{Candidate, Fragment, Graph, Namespace};
use crate::{EngineError, Match, Occurrence, Reach, Reason, ReferencesQuery};

/// A match that is a declaration: one with a symbol. Whether a path can
/// reach it — structs, functions and modules can be; methods, fields and
/// variants are reached through a type, which syntax alone cannot follow —
/// is the language's semantics, and only those can be a rename's target.
#[derive(Debug, Clone, Copy)]
pub struct Declaration<'a> {
    m: &'a Match,
}

impl<'a> Declaration<'a> {
    pub fn of(m: &'a Match) -> Option<Self> {
        m.symbol.as_ref().map(|_| Self { m })
    }

    /// The declarations of `language` in `matches` that a path can reach,
    /// as its semantics define it.
    pub fn addressable_in(
        matches: &'a [Match],
        language: &LanguageId,
        semantics: &vvv_core::Semantics,
    ) -> Vec<Self> {
        matches
            .iter()
            .filter(|m| &m.language == language)
            .filter_map(Self::of)
            .filter(|d| semantics.is_addressable(d.symbol().kind))
            .collect()
    }

    pub fn site(&self) -> DeclarationSite {
        DeclarationSite {
            path: self.m.path.clone(),
            start: self.m.start,
        }
    }

    pub fn path(&self) -> &'a Path {
        &self.m.path
    }

    pub fn language(&self) -> &'a LanguageId {
        &self.m.language
    }

    fn symbol(&self) -> &'a vvv_core::Symbol {
        self.m.symbol.as_ref().expect("a declaration has a symbol")
    }
}

/// The declaration a rename is about, once it is known which one.
pub struct Target {
    pub address: Address,
    /// The declaring language's module system: its layout resolves paths,
    /// its semantics judge them.
    pub ns: Namespace,
    /// Module addresses of the other declarations sharing the name.
    pub others: Vec<Address>,
    /// Every address that reaches this declaration through re-exports
    /// (`vvv_core::Span` for `vvv_core::text::span::Span`), the declaration's
    /// own first.
    pub aliases: Vec<Address>,
}

impl Target {
    /// Every token spelling `name` in `candidate`, judged against this
    /// declaration through what the file can see.
    pub fn judge(&self, candidate: &Candidate, name: &str) -> Result<Vec<Occurrence>, EngineError> {
        let tokens = candidate.references(name)?;
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        let scope = candidate.scope(&self.ns)?;
        Ok(tokens
            .into_iter()
            .map(|m| {
                let reason = scope.classify(&m, name, &self.aliases, &self.others);
                Occurrence::judged(m, reason)
            })
            .collect())
    }
}

/// Where a same-named declaration lives, for the ambiguity error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationSite {
    pub path: RelPath,
    pub start: Position,
}

impl std::fmt::Display for DeclarationSite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.path.display(), self.start.display())
    }
}

/// What a rename rests on: the declarations, every judged token, and the
/// languages in which more than one declaration shares the name.
pub struct Evidence {
    pub declarations: Vec<Match>,
    pub occurrences: Vec<Occurrence>,
    pub ambiguous: BTreeSet<LanguageId>,
}

impl Graph {
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
                    let Some(resolved) = &edge.address else {
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
            let Some(resolved) = &edge.address else {
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

impl Target {
    /// The one declaration meant among a language's `addressable` ones, with
    /// its address; `None` when there is none (methods, fields, variants) or
    /// the language has no layout. Several sharing the name need
    /// `declared_in` to choose. Declarations in other languages never
    /// compete: a Rust `foo` and a TypeScript `foo` are renamed side by side,
    /// each against its own target.
    pub fn of(
        graph: &mut Graph,
        query: &ReferencesQuery,
        addressable: &[Declaration<'_>],
    ) -> Result<Option<Target>, EngineError> {
        let chosen = match addressable {
            [] => return Ok(None),
            [only] => only,
            [first, ..] if query.declared_in.is_some() => first,
            many => {
                return Err(EngineError::AmbiguousSymbol {
                    name: query.name.clone(),
                    declarations: many.iter().map(Declaration::site).collect(),
                });
            }
        };
        let Some(ns) = graph.namespace(chosen.language()) else {
            return Ok(None);
        };
        let Ok(module) = ns.address(chosen.path()) else {
            return Ok(None);
        };
        // Other declarations of the same name anywhere in the language.
        let all = graph
            .search(&Query::named(query.name.as_str()).in_language(chosen.language().clone()))?
            .matches;
        let others = Declaration::addressable_in(&all, chosen.language(), ns.semantics())
            .into_iter()
            .filter(|d| d.path() != chosen.path())
            .filter_map(|d| ns.address(d.path()).ok())
            .collect();
        let address = module.join(query.name.as_str());
        Ok(Some(Target {
            aliases: vec![address.clone()],
            address,
            ns,
            others,
        }))
    }
}
