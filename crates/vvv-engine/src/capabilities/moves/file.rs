//! Moving a file or a directory: the [`MoveSet`] lists every file that
//! travels (a directory's contents, plus the layout's companions such as
//! Rust's `a.rs` beside `a/`); the [`Rebase`] knows the old and new address
//! of the moved thing, via the language's [`Layout`](vvv_core::Layout),
//! and re-renders every import that resolved under the old one — moved
//! files' own imports from their new locations. The language's
//! [`Surgery`](vvv_core::Surgery) adds whatever else it needs (Rust `mod`
//! declarations), from the parsed files the layout named.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vvv_core::{Address, Facts, Parsed, ReachKind, RelPath, ResolveError};

use super::{MoveSet, Reachability, Rebase, Widen};
use crate::change::Change;
use crate::command::{Command, Context};
use crate::protocol::vocabulary::IntentLine;
use crate::report::{Document, MoveCounts};
use crate::{
    EngineError, FileChange, Intent, Mutation, Notice, Planned, Respelling, SourceFile, VfsError,
};

/// Move a file and make every reference to it follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveIntent {
    pub from: PathBuf,
    pub to: PathBuf,
}

impl MoveIntent {
    pub fn new(from: impl Into<PathBuf>, to: impl Into<PathBuf>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

/// `vvv move`: a file or directory moved, its importers respelled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Move {
    pub intent: MoveIntent,
    pub applied: bool,
    /// The history entry the apply made, when `applied`; what `undo` reverses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_id: Option<u64>,
    /// Normalised, workspace-relative source and destination.
    pub from: RelPath,
    pub to: RelPath,
    /// The module address before and after, when the language has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_address: Option<Address>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_address: Option<Address>,
    /// References vvv found but could not rewrite.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
    /// References rewritten in place; every other edit in `files` is
    /// structural (a `mod` line moved, a visibility widened).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub respellings: Vec<Respelling>,
    pub files: Vec<FileChange>,
}

impl Mutation for Move {
    fn intent(&self) -> Intent {
        Intent::Move(self.intent.clone())
    }

    fn applied(&mut self, id: u64) {
        self.applied = true;
        self.history_id = Some(id);
    }
}

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
            change.merge(rewrite.change)?;
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
            let candidate = graph.file(&violation.path)?;
            let file = candidate.file();
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
                    Ok(edit) => change.edit(file.witness(), edit)?,
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
                let file = match graph.candidate(&path) {
                    Some(candidate) => candidate.file().clone(),
                    None => cx.workspace.load(&path)?,
                };
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
            let file = touched
                .iter()
                .find(|(file, _)| file.path() == side.path)
                .ok_or_else(|| ResolveError::Missing(side.path.clone().into()))?;
            change.edit(file.0.witness(), side.edit)?;
        }
        for (f, t) in moves.iter() {
            change.move_file(graph.file(f)?.file().witness(), t)?;
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

impl Document {
    pub(crate) fn move_file(result: &Move) -> Self {
        let mut report = Self::new();
        // Normalised paths, not the ones typed: what history will show.
        report.title(IntentLine(&Intent::Move(MoveIntent::new(
            &result.from,
            &result.to,
        ))));
        let structural = report.moved(&result.files, &result.respellings, &result.notices);
        report.moved_summary(
            result.applied,
            result.history_id,
            MoveCounts {
                respellings: result.respellings.len(),
                structural,
                notices: result.notices.len(),
                files: result.files.len(),
            },
        );
        report
    }
}
