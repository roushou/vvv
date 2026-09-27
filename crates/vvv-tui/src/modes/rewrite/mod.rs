//! Rewrite state and ticked matches.
pub(crate) mod screen;
mod update;
use crate::model::{Cursor, Panels};
use std::collections::BTreeSet;
use std::path::Path;
use vvv_engine::protocol::FileChange;
use vvv_engine::{Match, MatchId, Query};
// ---------------------------------------------------------------- rewrite

/// A rewrite being shaped: the search's matches, the template edited live,
/// and what each match becomes.
#[derive(Debug)]
pub struct RewriteMode {
    pub query: Query,
    pub template: String,
    pub matches: Vec<Match>,
    /// The last plan's files, each holding its diff: the preview the detail
    /// pane draws.
    pub changes: Vec<FileChange>,
    pub ticks: BTreeSet<MatchId>,
    pub focus: RewritePanel,
    pub cursor: Cursor,
    pub detail_scroll: usize,
    pub error: Option<String>,
    pub busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewritePanel {
    Template,
    Matches,
    Detail,
}

impl Panels for RewritePanel {
    const ALL: &'static [Self] = &[Self::Template, Self::Matches, Self::Detail];
}

impl RewriteMode {
    pub fn new(query: Query, matches: Vec<Match>) -> Self {
        let ticks = matches.iter().map(|m| m.id.clone()).collect();
        Self {
            query,
            template: String::new(),
            matches,
            changes: Vec::new(),
            ticks,
            focus: RewritePanel::Template,
            cursor: Cursor::default(),
            detail_scroll: 0,
            error: None,
            busy: false,
        }
    }

    pub fn intent(&self) -> Option<vvv_engine::RewriteIntent> {
        (!self.template.trim().is_empty())
            .then(|| vvv_engine::RewriteIntent::new(self.query.clone(), self.template.as_str()))
    }

    pub fn current(&self) -> Option<&Match> {
        self.matches.get(self.cursor.index)
    }

    pub fn is_ticked(&self, m: &Match) -> bool {
        self.ticks.contains(&m.id)
    }

    pub fn toggle(&mut self) {
        if let Some(id) = self.current().map(|m| m.id.clone())
            && !self.ticks.remove(&id)
        {
            self.ticks.insert(id);
        }
    }

    pub fn toggle_all(&mut self) {
        if self.ticks.len() == self.matches.len() {
            self.ticks.clear();
        } else {
            self.ticks = self.matches.iter().map(|m| m.id.clone()).collect();
        }
    }

    pub fn files(&self) -> usize {
        self.matches
            .iter()
            .filter(|m| self.is_ticked(m))
            .map(|m| m.path.as_path())
            .collect::<BTreeSet<&Path>>()
            .len()
    }
}
