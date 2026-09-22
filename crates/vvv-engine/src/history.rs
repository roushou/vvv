//! Writing and unwriting: [`Apply`] turns a planned result into files and one
//! entry of the undo stack — one JSON file under `.vvv/` holding the receipts
//! of the most recent applies, newest last — [`UndoLast`] reverses the newest,
//! [`HistoryQuery`] lists them.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::command::{Command, Context};
use crate::{
    EngineError, History as HistoryResult, HistoryEntry, HistoryQuery, Intent, Mutation, Planned,
    Receipt, Undo, UndoLast, VfsError, Workspace,
};

const FILE: &str = ".vvv/history.json";
/// Receipts carry full file contents; keep the stack short.
const LIMIT: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error(transparent)]
    Vfs(#[from] VfsError),
    #[error(
        "{FILE} is not readable as history ({0}); if it was written by an older vvv, delete it"
    )]
    Corrupt(String),
    #[error("nothing to undo")]
    Empty,
}

/// What the file holds per apply: the entry a client sees plus the receipt
/// that undoes it. The receipt never leaves the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    pub id: u64,
    /// Seconds since the Unix epoch.
    pub at: u64,
    pub intent: Intent,
    pub receipt: Receipt,
}

impl Record {
    pub fn entry(&self) -> HistoryEntry {
        HistoryEntry {
            id: self.id,
            at: self.at,
            intent: self.intent.clone(),
            files: self.receipt.paths().count(),
            paths: self.receipt.paths().map(Into::into).collect(),
            moves: self
                .receipt
                .moves()
                .iter()
                .map(|(f, t)| (f.into(), t.into()))
                .collect(),
        }
    }
}

pub(crate) struct History<'a> {
    workspace: &'a Workspace,
}

impl<'a> History<'a> {
    pub fn new(workspace: &'a Workspace) -> Self {
        Self { workspace }
    }

    pub fn path() -> &'static Path {
        Path::new(FILE)
    }

    fn file(&self) -> PathBuf {
        self.workspace.absolute(Self::path())
    }

    pub fn entries(&self) -> Result<Vec<Record>, HistoryError> {
        let vfs = self.workspace.vfs();
        let file = self.file();
        if !vfs.exists(&file) {
            return Ok(Vec::new());
        }
        serde_json::from_str(&vfs.read(&file)?).map_err(|e| HistoryError::Corrupt(e.to_string()))
    }

    fn save(&self, entries: &[Record]) -> Result<(), HistoryError> {
        let text =
            serde_json::to_string(entries).map_err(|e| HistoryError::Corrupt(e.to_string()))?;
        Ok(self.workspace.vfs().write(&self.file(), &text)?)
    }

    pub fn push(&self, intent: Intent, receipt: Receipt) -> Result<Record, HistoryError> {
        let mut entries = self.entries()?;
        let entry = Record {
            id: entries.last().map_or(1, |e| e.id + 1),
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            intent,
            receipt,
        };
        entries.push(entry.clone());
        if entries.len() > LIMIT {
            entries.drain(..entries.len() - LIMIT);
        }
        self.save(&entries)?;
        Ok(entry)
    }

    /// The newest entry, without removing it.
    pub fn last(&self) -> Result<Record, HistoryError> {
        self.entries()?.pop().ok_or(HistoryError::Empty)
    }

    /// Remove the newest entry. Called after its receipt was undone.
    pub fn pop(&self) -> Result<(), HistoryError> {
        let mut entries = self.entries()?;
        entries.pop().ok_or(HistoryError::Empty)?;
        self.save(&entries)
    }
}

/// Write what was planned and record it as one history entry — one undo.
/// A batch's steps go in order, each checked against the state the previous
/// one left; if a step fails, the ones before it are rolled back. Answers
/// with the result marked applied.
pub struct Apply<T>(pub Planned<T>);

impl<T: Mutation> Command for Apply<T> {
    type Output = T;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let workspace = cx.workspace;
        // Whatever happens below, the tree is no longer what the graph saw.
        cx.graph.touched();
        let (mut result, plans) = self.0.into_parts();
        let mut receipt: Option<Receipt> = None;
        for plan in plans {
            match plan.apply(workspace) {
                Ok(applied) => {
                    receipt = Some(match receipt.take() {
                        Some(so_far) => so_far.then(applied),
                        None => applied,
                    });
                }
                Err(error) => {
                    if let Some(so_far) = receipt {
                        let _ = so_far.rollback(workspace);
                    }
                    return Err(error.into());
                }
            }
        }
        let record =
            History::new(cx.workspace).push(result.intent(), receipt.unwrap_or_default())?;
        result.applied(record.id);
        Ok(result)
    }
}

/// Reverse the most recent apply, provided its files are untouched since.
impl Command for UndoLast {
    type Output = Undo;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let history = History::new(cx.workspace);
        let record = history.last()?;
        cx.graph.touched();
        record.receipt.undo(cx.workspace)?;
        history.pop()?;
        Ok(Undo {
            undone: record.entry(),
            restored: record.receipt.paths().map(Into::into).collect(),
            moves_reverted: record
                .receipt
                .moves()
                .iter()
                .map(|(f, t)| (f.into(), t.into()))
                .collect(),
        })
    }
}

/// Applies that can still be undone, oldest first.
impl Command for HistoryQuery {
    type Output = HistoryResult;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let records = History::new(cx.workspace).entries()?;
        Ok(HistoryResult {
            entries: records.iter().map(Record::entry).collect(),
        })
    }
}
