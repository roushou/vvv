//! After a move, does every reference still reach what it names? A rule over
//! addresses and reaches: for each affected reference, every module along the
//! path to its target and the target itself must admit the module it is read
//! from. Where one does not, the narrowest idiomatic widening that covers all
//! its consumers — or, across packages, a notice.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::Reach;

use vvv_core::{Address, Layout, Project, ReachKind, Semantics, Symbol, SymbolKind};

use crate::EngineError;
use crate::graph::Graph;

/// A declaration some consumer can no longer see, and what it would take.
#[derive(Debug, Clone)]
pub struct Violation {
    /// The file declaring it, at its current path.
    pub path: PathBuf,
    pub symbol: Symbol,
    /// The module the declaration belongs to after the move.
    pub declaring: Address,
    pub consumers: Vec<Address>,
    /// The idiomatic reach that covers them: `Parent`, `Package`, or
    /// `Everyone` when a consumer is in another package.
    pub needs: ReachKind,
}

pub struct Reachability<'a> {
    layout: &'a dyn Layout,
    semantics: &'static Semantics,
    project: &'a Project,
    /// The moved address before and after, to find a post-move address's
    /// declaration in the tree as it is now.
    old: &'a Address,
    new: &'a Address,
}

impl<'a> Reachability<'a> {
    pub fn new(
        layout: &'a dyn Layout,
        semantics: &'static Semantics,
        project: &'a Project,
        old: &'a Address,
        new: &'a Address,
    ) -> Self {
        Self {
            layout,
            semantics,
            project,
            old,
            new,
        }
    }

    /// Judge every (consumer module, target) pair; one violation per
    /// declaration, with every consumer that cannot see it.
    pub fn check(
        &self,
        graph: &Graph,
        references: &[(Address, Address)],
    ) -> Result<Vec<Violation>, EngineError> {
        // Declaration → (its current file, its symbol, its module after the move, consumers).
        let mut offended: BTreeMap<Address, (PathBuf, Symbol, Address, Vec<Address>)> =
            BTreeMap::new();
        let mut seen: BTreeMap<Address, Option<(PathBuf, Symbol)>> = BTreeMap::new();
        for (from, target) in references {
            for step in Self::chain(target) {
                let Some(declaring) = step.parent() else {
                    continue; // a package root declares itself
                };
                let found = match seen.get(&step) {
                    Some(found) => found.clone(),
                    None => {
                        let found = self.declaration(graph, &step, step == *target)?;
                        seen.insert(step.clone(), found.clone());
                        found
                    }
                };
                let Some((path, symbol)) = found else {
                    continue; // nothing declares it in a way vvv can read (a directory module, say)
                };
                let reach = Reach::of(
                    self.semantics.reach_kind(symbol.modifier()),
                    &declaring,
                    None,
                );
                if reach.admits(from) {
                    continue;
                }
                let entry = offended
                    .entry(step.clone())
                    .or_insert_with(|| (path, symbol, declaring, Vec::new()));
                if !entry.3.contains(from) {
                    entry.3.push(from.clone());
                }
            }
        }
        Ok(offended
            .into_values()
            .map(|(path, symbol, declaring, consumers)| {
                let needs = Reach::narrowest(&declaring, &consumers);
                Violation {
                    path,
                    symbol,
                    declaring,
                    consumers,
                    needs,
                }
            })
            .collect())
    }

    /// Every address on the way to `target`, from the first module below the
    /// package root down to the target itself.
    fn chain(target: &Address) -> impl Iterator<Item = Address> + '_ {
        (1..=target.path().len())
            .map(|n| Address::new(target.package().clone(), target.path()[..n].iter().cloned()))
    }

    /// The declaration of `after` — a post-move address — in the tree as it
    /// is now: the `mod` line or item in its parent module's file. `last`
    /// says whether `after` is the reference's target (may be any item) or a
    /// module on the way (must be one).
    fn declaration(
        &self,
        graph: &Graph,
        after: &Address,
        last: bool,
    ) -> Result<Option<(PathBuf, Symbol)>, EngineError> {
        // Where it is declared today: the same place, unless it moved.
        let before = after
            .rebase(self.new, self.old)
            .unwrap_or_else(|| after.clone());
        let Some(parent) = before.parent() else {
            return Ok(None);
        };
        let Some(name) = before.path().last() else {
            return Ok(None);
        };
        let Some(file) = self
            .layout
            .candidates(self.project, &parent)
            .into_iter()
            .find(|p| self.project.files.contains(p))
        else {
            return Ok(None);
        };
        let Some(candidate) = graph.candidate(&file) else {
            return Ok(None);
        };
        let facts = candidate.facts()?;
        let symbol = facts
            .symbols
            .iter()
            .filter(|s| s.name == name.as_str())
            .find(|s| {
                s.kind == SymbolKind::Module || (last && self.semantics.is_addressable(s.kind))
            })
            .cloned();
        Ok(symbol.map(|s| (file, s)))
    }
}
