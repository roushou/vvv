//! Reference evidence and target selection: declaration sites, the target's
//! namespace and aliases, and token judgments through the candidate's scope.
//! Graph owns the tree queries that gather this evidence.

use std::collections::BTreeSet;
use std::path::Path;

use vvv_core::RelPath;
use vvv_core::{Address, LanguageId, Position, Query};

use super::{Candidate, Graph, Namespace};
use crate::{EngineError, Match, Occurrence, ReferencesQuery};

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
