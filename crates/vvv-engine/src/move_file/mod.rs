//! Moving a file or a directory: the [`MoveSet`] lists every file that
//! travels (a directory's contents, plus the layout's companions such as
//! Rust's `a.rs` beside `a/`); the [`Rebase`] knows the old and new address
//! of the moved thing, via the language's [`Layout`](vvv_core::Layout),
//! and re-renders every import that resolved under the old one — moved
//! files' own imports from their new locations. The language's
//! [`Surgery`](vvv_core::Surgery) adds whatever else it needs (Rust `mod`
//! declarations), from the parsed files the layout named.

mod extraction;
mod reachability;
mod rebase;
mod set;
mod symbol;
mod widen;

pub(crate) use extraction::Extraction;
pub(crate) use reachability::Reachability;
pub(crate) use rebase::Rebase;
pub(crate) use set::MoveSet;
pub(crate) use widen::Widen;

use std::path::Path;

use vvv_core::{Address, Facts, Parsed, ReachKind, ResolveError};

use crate::change::Change;
use crate::command::{Command, Context};
use crate::{EngineError, Move, MoveIntent, Planned, SourceFile, VfsError};

/// Plan moving a file or a directory. Nothing is written; see
/// [`Apply`](crate::Apply). A directory moves with everything under it; a
/// Rust module moves with its `a.rs` + `a/` pair either way.
impl Command for MoveIntent {
    type Output = Planned<Move>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let intent = &self;
        let from = cx.workspace.normalize(&intent.from);
        let to = cx.workspace.normalize(&intent.to);
        if to.starts_with(&from) {
            return Err(ResolveError::IntoItself {
                from: from.into(),
                to: to.into(),
            }
            .into());
        }
        let graph = &mut *cx.graph;
        let language = graph
            .language_of(&from)
            .or_else(|| graph.language_of_directory(&from))
            .ok_or_else(|| EngineError::NoLanguage(from.clone().into()))?;
        let ns = graph
            .namespace(&language.id())
            .ok_or_else(|| EngineError::NoLayout(language.id()))?;
        let (layout, surgery) = (ns.layout(), ns.surgery()?);
        let project = ns.project().clone();
        let candidates = graph.files(Some(&language.id()));
        let moves = MoveSet::compute(cx.workspace, layout, &project, &from, &to)?;
        if moves.is_empty() {
            return Err(VfsError::NotFound(from).into());
        }
        for (f, t) in moves.iter() {
            let same_language = |p: &Path| {
                graph
                    .language_of(p)
                    .is_some_and(|l| l.id() == language.id())
            };
            if !same_language(f) {
                return Err(EngineError::NoLanguage(f.into()));
            }
            if !same_language(t) {
                return Err(EngineError::NoLanguage(t.into()));
            }
            if cx.workspace.vfs().exists(&cx.workspace.absolute(t)) {
                return Err(EngineError::Exists(t.into()));
            }
        }

        let rebase = Rebase::new(layout, surgery, &project, &from, &to, &moves)?;
        let mut change = Change::new();
        let mut references: Vec<(Address, Address)> = Vec::new();
        for candidate in &candidates {
            let rewrite = rebase.rewrite(candidate)?;
            change.merge(rewrite.change);
            references.extend(rewrite.references);
        }
        references.sort();
        references.dedup();

        // Can every reference still see what it names? Widen what needs it,
        // as narrowly as real code writes; across a package boundary, say so.
        let old = layout.address(&project, &from)?;
        let new = layout.address(&project, &to)?;
        let violations = Reachability::new(layout, ns.semantics(), &project, &old, &new)
            .check(graph, &references)?;
        let mut moved_mod_needs: Option<ReachKind> = None;
        for violation in violations {
            let is_moved_mod = violation.symbol.kind == vvv_core::SymbolKind::Module
                && violation.declaring.join(violation.symbol.name.as_str()) == new;
            if is_moved_mod && violation.needs != ReachKind::Everyone {
                // Its line is being moved by `relocate`; tell it what to write.
                moved_mod_needs = Some(violation.needs);
                continue;
            }
            let file = cx.workspace.load(&violation.path)?;
            let at = (
                violation.path.clone(),
                file.source().position(violation.symbol.name_span.start),
            );
            // Across a package boundary every consumer is told; otherwise one
            // widening serves them all.
            let consumers: Vec<Address> = if violation.needs == ReachKind::Everyone {
                violation.consumers.clone()
            } else {
                vec![
                    violation
                        .consumers
                        .first()
                        .cloned()
                        .unwrap_or_else(|| new.clone()),
                ]
            };
            for consumer in consumers {
                let widen = Widen {
                    symbol: &violation.symbol,
                    source: file.source(),
                    needs: violation.needs,
                    consumer,
                    notice_at: at.clone(),
                };
                match widen.plan(surgery) {
                    Ok(edit) => change.edit(&violation.path, edit),
                    Err(notice) => change.notice(notice),
                }
            }
        }

        // What else the move changes — Rust's `mod` lines — the layout names
        // and the surgery writes, from the parsed files, never from disk.
        let touched: Vec<(SourceFile, Facts)> = layout
            .touched_by_move(&project, &from, &to)?
            .into_iter()
            .map(|path| {
                let file = cx.workspace.load(&path)?;
                let facts = language
                    .facts(file.text())
                    .map_err(|source| EngineError::Search {
                        path: path.clone().into(),
                        source,
                    })?;
                Ok((file, facts))
            })
            .collect::<Result<_, EngineError>>()?;
        let parsed: Vec<Parsed<'_>> = touched
            .iter()
            .map(|(file, facts)| Parsed {
                path: file.path(),
                source: file.source(),
                facts,
            })
            .collect();
        for side in surgery.relocate(&project, &from, &to, &parsed, moved_mod_needs)? {
            change.edit(side.path, side.edit);
        }
        for (f, t) in moves.iter() {
            change.move_file(f, t);
        }
        Planned::of(cx.workspace, change, |bound, files| Move {
            intent: intent.clone(),
            applied: false,
            history_id: None,
            from: from.into(),
            to: to.into(),
            from_address: Some(old),
            to_address: Some(new),
            notices: bound.notices,
            respellings: bound.respellings,
            files,
        })
    }
}
