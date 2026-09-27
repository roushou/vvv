//! Rename state and the operations its panels can perform.
pub(crate) mod screen;
mod update;
use crate::model::{Cursor, FilePreview, Panels};
use std::collections::BTreeSet;
use std::path::Path;
use vvv_engine::protocol::FileChange;
use vvv_engine::{Confidence, Match, MatchId, Occurrence, RelPath, SymbolKind};

/// What `r` found under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTarget {
    pub name: String,
    pub symbol: Option<SymbolKind>,
    pub declared_in: Option<RelPath>,
}

// ---------------------------------------------------------------- rename

/// A rename being judged: the new name, every occurrence by verdict, and
/// which of them are ticked for the commit.
#[derive(Debug)]
pub struct RenameMode {
    pub target: RenameTarget,
    pub language: Option<vvv_engine::LanguageId>,
    /// The new name, edited live; the plan is re-made as it grows.
    pub name: String,
    pub declarations: Vec<Match>,
    pub occurrences: Vec<Occurrence>,
    /// The last plan's files, each holding its diff: the preview the detail
    /// pane draws.
    pub changes: Vec<FileChange>,
    pub ticks: BTreeSet<MatchId>,
    pub focus: RenamePanel,
    /// Cursors of the `?`, `✓` and `✗` panels, in that order.
    pub cursors: [Cursor; 3],
    /// The list panel the detail follows when focus is elsewhere.
    pub last_list: RenamePanel,
    pub detail_scroll: usize,
    pub preview: Option<FilePreview>,
    /// The verdicts are in and the ticks seeded; later plans only refresh
    /// `changes`, so typing does not undo the user's ticks.
    pub judged: bool,
    /// Waiting for the judge, or for the commit.
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenamePanel {
    Name,
    Unsure,
    Sure,
    Other,
    Detail,
}

impl Panels for RenamePanel {
    const ALL: &'static [Self] = &[
        Self::Name,
        Self::Unsure,
        Self::Sure,
        Self::Other,
        Self::Detail,
    ];
}

impl RenamePanel {
    pub fn confidence(self) -> Option<Confidence> {
        match self {
            Self::Unsure => Some(Confidence::Unresolved),
            Self::Sure => Some(Confidence::Resolved),
            Self::Other => Some(Confidence::Other),
            Self::Name | Self::Detail => None,
        }
    }

    fn slot(self) -> Option<usize> {
        match self {
            Self::Unsure => Some(0),
            Self::Sure => Some(1),
            Self::Other => Some(2),
            Self::Name | Self::Detail => None,
        }
    }
}

impl RenameMode {
    pub fn new(target: RenameTarget, language: Option<vvv_engine::LanguageId>) -> Self {
        Self {
            name: String::new(),
            target,
            language,
            declarations: Vec::new(),
            occurrences: Vec::new(),
            changes: Vec::new(),
            ticks: BTreeSet::new(),
            focus: RenamePanel::Name,
            cursors: [Cursor::default(); 3],
            last_list: RenamePanel::Unsure,
            detail_scroll: 0,
            preview: None,
            judged: false,
            busy: true,
            error: None,
        }
    }

    /// The rows of one verdict panel, in the engine's order.
    pub fn rows(&self, confidence: Confidence) -> Vec<&Occurrence> {
        self.occurrences
            .iter()
            .filter(|o| o.confidence == confidence)
            .collect()
    }

    pub fn cursor(&self, panel: RenamePanel) -> Option<&Cursor> {
        panel.slot().map(|i| &self.cursors[i])
    }

    pub fn cursor_mut(&mut self, panel: RenamePanel) -> Option<&mut Cursor> {
        panel.slot().map(move |i| &mut self.cursors[i])
    }

    /// The list panel whose row the detail explains.
    pub fn list(&self) -> RenamePanel {
        if self.focus.confidence().is_some() {
            self.focus
        } else {
            self.last_list
        }
    }

    pub fn current(&self) -> Option<&Occurrence> {
        let panel = self.list();
        let rows = self.rows(panel.confidence()?);
        rows.get(self.cursor(panel)?.index).copied()
    }

    pub fn is_ticked(&self, o: &Occurrence) -> bool {
        self.ticks.contains(&o.m.id)
    }

    pub fn ticked(&self, confidence: Confidence) -> usize {
        self.rows(confidence)
            .iter()
            .filter(|o| self.is_ticked(o))
            .count()
    }

    pub fn toggle(&mut self) {
        if let Some(id) = self.current().map(|o| o.m.id.clone())
            && !self.ticks.remove(&id)
        {
            self.ticks.insert(id);
        }
    }

    /// Tick every row of the focused panel, or untick them all when they
    /// already are.
    pub fn toggle_panel(&mut self) {
        let Some(confidence) = self.list().confidence() else {
            return;
        };
        let ids: Vec<MatchId> = self
            .rows(confidence)
            .iter()
            .map(|o| o.m.id.clone())
            .collect();
        if ids.iter().all(|id| self.ticks.contains(id)) {
            for id in &ids {
                self.ticks.remove(id);
            }
        } else {
            self.ticks.extend(ids);
        }
    }

    /// The plan's change for the occurrence's file when the plan edits this
    /// very site: the diff the detail pane draws. A file with only other
    /// sites changed is not this row's preview.
    pub fn file(&self, o: &Occurrence) -> Option<&FileChange> {
        self.changes
            .iter()
            .find(|f| f.path == o.m.path && f.edits.iter().any(|e| e.span == o.m.span))
    }

    /// Files the commit touches, from the ticks.
    pub fn files(&self) -> usize {
        self.occurrences
            .iter()
            .filter(|o| self.is_ticked(o))
            .map(|o| o.m.path.as_path())
            .collect::<BTreeSet<&Path>>()
            .len()
    }
}
