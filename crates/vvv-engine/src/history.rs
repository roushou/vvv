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

    fn snapshot(&self) -> Result<HistorySnapshot, HistoryError> {
        let original = match self.workspace.vfs().read(&self.file()) {
            Ok(text) => Some(text),
            Err(VfsError::NotFound(_)) => None,
            Err(error) => return Err(error.into()),
        };
        let entries = original
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| HistoryError::Corrupt(e.to_string()))?
            .unwrap_or_default();
        Ok(HistorySnapshot { original, entries })
    }

    pub fn entries(&self) -> Result<Vec<Record>, HistoryError> {
        Ok(self.snapshot()?.entries)
    }
}

/// Validated history retained for one mutation, including its exact before-state.
struct HistorySnapshot {
    original: Option<String>,
    entries: Vec<Record>,
}

impl HistorySnapshot {
    fn save(&self, transaction: &mut crate::plan::Transaction<'_>) -> Result<(), HistoryError> {
        let text = serde_json::to_string(&self.entries)
            .map_err(|error| HistoryError::Corrupt(error.to_string()))?;
        Ok(transaction.save_file(&History::path().into(), self.original.as_deref(), &text)?)
    }

    fn push(
        &mut self,
        id: u64,
        intent: Intent,
        receipt: Receipt,
        transaction: &mut crate::plan::Transaction<'_>,
    ) -> Result<Record, HistoryError> {
        let entry = Record {
            id,
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            intent,
            receipt,
        };
        self.entries.push(entry.clone());
        if self.entries.len() > LIMIT {
            self.entries.drain(..self.entries.len() - LIMIT);
        }
        self.save(transaction)?;
        Ok(entry)
    }

    fn last(&self) -> Result<&Record, HistoryError> {
        self.entries.last().ok_or(HistoryError::Empty)
    }

    fn pop(&mut self, transaction: &mut crate::plan::Transaction<'_>) -> Result<(), HistoryError> {
        self.entries.pop().ok_or(HistoryError::Empty)?;
        self.save(transaction)
    }

    fn next_id(&self) -> Result<u64, HistoryError> {
        self.entries.last().map_or(Ok(1), |entry| {
            entry
                .id
                .checked_add(1)
                .ok_or_else(|| HistoryError::Corrupt("history entry id is exhausted".to_owned()))
        })
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
        let history = History::new(workspace);
        let mut snapshot = history.snapshot()?;
        let id = snapshot.next_id()?;
        // Whatever happens below, the tree is no longer what the graph saw.
        cx.graph.touched();
        let (mut result, plans) = self.0.into_parts();
        let mut transaction = crate::plan::Transaction::new(workspace);
        for plan in plans {
            if let Err(error) = transaction.apply(plan) {
                return Err(transaction.recover(error.into()));
            }
        }
        let receipt = transaction.receipt();
        let record = match snapshot.push(id, result.intent(), receipt, &mut transaction) {
            Ok(record) => record,
            Err(error) => return Err(transaction.recover(error.into())),
        };
        result.applied(record.id);
        Ok(result)
    }
}

/// Reverse the most recent apply, provided its files are untouched since.
impl Command for UndoLast {
    type Output = Undo;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let history = History::new(cx.workspace);
        let mut snapshot = history.snapshot()?;
        let record = snapshot.last()?.clone();
        cx.graph.touched();
        let mut transaction = crate::plan::Transaction::new(cx.workspace);
        if let Err(error) = record.receipt.undo_in(&mut transaction) {
            return Err(transaction.recover(error.into()));
        }
        if let Err(error) = snapshot.pop(&mut transaction) {
            return Err(transaction.recover(error.into()));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_ids_never_wrap() {
        let snapshot = HistorySnapshot {
            original: None,
            entries: vec![Record {
                id: u64::MAX,
                at: 0,
                intent: Intent::Rewrite(crate::RewriteIntent::new(crate::Query::pattern("a"), "b")),
                receipt: Receipt::default(),
            }],
        };
        assert!(matches!(snapshot.next_id(), Err(HistoryError::Corrupt(_))));
    }
}
