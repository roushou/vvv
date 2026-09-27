//! Writing and unwriting: [`Apply`] turns a planned result into files and one
//! entry of the undo stack — one JSON file under `.vvv/` holding the receipts
//! of the most recent applies, newest last — [`Ledger::undo`] reverses the newest,
//! [`Ledger::history`] lists them.

use crate::History;
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{IntentLine, Mark, Plural};
use crate::report::{Block, Document};

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    EngineError, History as HistoryResult, HistoryEntry, Intent, Mutation, Planned, Receipt, Undo,
    VfsError,
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

pub struct Ledger<'a> {
    engine: &'a crate::Engine,
}

impl<'a> Ledger<'a> {
    pub fn new(engine: &'a crate::Engine) -> Self {
        Self { engine }
    }

    pub fn path() -> &'static Path {
        Path::new(FILE)
    }

    fn file(&self) -> PathBuf {
        self.engine.workspace().absolute(Self::path())
    }

    fn snapshot(&self) -> Result<HistorySnapshot, HistoryError> {
        let original = match self.engine.workspace().vfs().read(&self.file()) {
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

    fn entries(&self) -> Result<Vec<Record>, HistoryError> {
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
        Ok(transaction.save_file(&Ledger::path().into(), self.original.as_deref(), &text)?)
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
/// Query answers cannot be applied:
///
/// ```compile_fail,E0277
/// use vvv_engine::{Answer, Apply};
/// let _: Option<Apply<Answer>> = None;
/// ```
pub struct Apply<T: Mutation>(pub Planned<T>);

impl<T: Mutation> Apply<T> {
    /// Apply a retained typed plan and commit its history entry.
    pub fn apply(self, engine: &crate::Engine) -> Result<Applied<T>, EngineError> {
        let _operation = engine.operation();
        self.apply_in(engine)
    }

    pub(crate) fn apply_in(self, engine: &crate::Engine) -> Result<Applied<T>, EngineError> {
        let workspace = engine.workspace();
        let history = Ledger::new(engine);
        let mut snapshot = history.snapshot()?;
        let id = snapshot.next_id()?;
        // Whatever happens below, the tree is no longer what the graph saw.
        engine.touched();
        let (intent, result, plans) = self.0.into_parts();
        let mut transaction = crate::plan::Transaction::new(workspace);
        for plan in plans {
            if let Err(error) = transaction.apply(plan) {
                return Err(transaction.recover(error.into()));
            }
        }
        let receipt = transaction.receipt();
        let record = match snapshot.push(id, intent, receipt, &mut transaction) {
            Ok(record) => record,
            Err(error) => return Err(transaction.recover(error.into())),
        };
        Ok(Applied::new(result, record.id))
    }
}

/// A successful apply with the required id of its committed history entry.
/// Construction is private and result access is immutable.
///
/// ```compile_fail,E0596
/// use vvv_engine::{Applied, Rename};
/// let change_description = |mut applied: Applied<Rename>| {
///     applied.intent.to.clear();
/// };
/// ```
#[derive(Debug, Clone)]
pub struct Applied<T: Mutation> {
    result: T,
    history_id: u64,
}

impl<T: Mutation> Applied<T> {
    fn new(mut result: T, history_id: u64) -> Self {
        result.applied(history_id);
        Self { result, history_id }
    }

    /// The history entry committed by this apply.
    pub fn history_id(&self) -> u64 {
        self.history_id
    }

    /// Preserve the committed id while widening only to the closed mutation sum.
    pub fn into_mutation(self) -> Applied<crate::MutationAnswer> {
        Applied {
            result: self.result.into_mutation(),
            history_id: self.history_id,
        }
    }

    /// The applied wire payload without its in-process completion handle.
    pub fn into_inner(self) -> T {
        self.result
    }
}

impl<T: Mutation> std::ops::Deref for Applied<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.result
    }
}

impl<T: Mutation + Serialize> Serialize for Applied<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.result.serialize(serializer)
    }
}

impl Ledger<'_> {
    /// Reverse the latest apply, provided its files are untouched since.
    pub fn undo(&self) -> Result<Undo, EngineError> {
        let _operation = self.engine.operation();
        self.undo_in()
    }

    pub(crate) fn undo_in(&self) -> Result<Undo, EngineError> {
        let history = self;
        let mut snapshot = history.snapshot()?;
        let record = snapshot.last()?.clone();
        self.engine.touched();
        let mut transaction = crate::plan::Transaction::new(self.engine.workspace());
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
    /// Read applies that can still be undone, oldest first.
    pub fn history(&self) -> Result<HistoryResult, EngineError> {
        let _operation = self.engine.operation();
        self.history_in()
    }

    pub(crate) fn history_in(&self) -> Result<HistoryResult, EngineError> {
        let records = self.entries()?;
        Ok(HistoryResult {
            entries: records.iter().map(Record::entry).collect(),
        })
    }
}

impl Document {
    pub(crate) fn undo(result: &Undo) -> Self {
        let mut report = Self::new();
        report.title(format!(
            "{} #{}  {}",
            Mark::Undo.glyph(),
            result.undone.id,
            IntentLine(&result.undone.intent)
        ));
        report.block_body(Block::Undo {
            moves: result.moves_reverted.clone(),
            restored: result.restored.clone(),
        });
        report.block_note(Block::Summary(
            Line::mark(Mark::Undo)
                .and(Role::Plain, " ")
                .and(Role::Plain, format!("#{}   ", result.undone.id))
                .and(Role::Dim, Plural(result.restored.len(), "file").to_string()),
        ));
        report
    }

    pub(crate) fn history(result: &History) -> Self {
        let mut report = Self::new();
        if result.entries.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no history"),
            ));
            return report;
        }
        let last = result.entries.len() - 1;
        report.block_body(Block::History(result.entries.clone()));
        report.block_note(Block::Summary(
            Line::of(
                Role::Plain,
                Plural(result.entries.len(), "entry").to_string(),
            )
            .and(Role::Plain, "   ")
            .and_line(Line::mark(Mark::Undo))
            .and(Role::Plain, format!(" #{}", result.entries[last].id)),
        ));
        report
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
