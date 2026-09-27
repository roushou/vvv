//! File and symbol move state.
pub(crate) mod screen;
mod update;
use crate::model::{Cursor, FilePreview, Panels};
use vvv_engine::protocol::FileChange;
use vvv_engine::{Intent, Notice, RelPath, Respelling};
// ---------------------------------------------------------------- move

/// A move being planned: the destination, edited live, and the plan it
/// yields — or why it yields none.
#[derive(Debug)]
pub struct MoveMode {
    pub from: RelPath,
    /// `Some` when one declaration moves rather than the file.
    pub symbol: Option<String>,
    pub to: String,
    pub plan: Option<MovePlan>,
    pub error: Option<String>,
    pub focus: MovePanel,
    /// Cursors of the `→`, `±` and `!` panels.
    pub cursors: [Cursor; 3],
    pub last_list: MovePanel,
    pub detail_scroll: usize,
    pub preview: Option<FilePreview>,
    /// Show the whole file diff in the detail panel.
    pub diff: bool,
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub struct MovePlan {
    pub intent: Intent,
    pub files: Vec<FileChange>,
    pub respellings: Vec<Respelling>,
    pub notices: Vec<Notice>,
    /// Indices into `files` of the structural changes: moved, or holding an
    /// edit no respelling accounts for.
    pub structural: Vec<usize>,
}

impl MovePlan {
    pub fn new(
        intent: Intent,
        files: Vec<FileChange>,
        respellings: Vec<Respelling>,
        notices: Vec<Notice>,
    ) -> Self {
        let structural = files
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                f.moved_to.is_some()
                    || f.edits.is_empty()
                    || !f.edits.iter().all(|e| {
                        respellings
                            .iter()
                            .any(|r| r.path == f.path && r.span == e.span)
                    })
            })
            .map(|(i, _)| i)
            .collect();
        Self {
            intent,
            files,
            respellings,
            notices,
            structural,
        }
    }

    /// One line per structural file: where it goes, or its first changed line.
    pub fn structural_label(&self, i: usize) -> String {
        let file = &self.files[i];
        if let Some(to) = &file.moved_to {
            return format!("{} → {}", file.path.short(), to.short());
        }
        let change = file
            .diff
            .as_str()
            .lines()
            .find(|l| {
                (l.starts_with('-') || l.starts_with('+'))
                    && !l.starts_with("---")
                    && !l.starts_with("+++")
            })
            .map(|l| format!("{} {}", &l[..1], l[1..].trim()))
            .unwrap_or_default();
        format!("{}  {change}", file.path.short())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovePanel {
    To,
    Respellings,
    Structural,
    Notices,
    Detail,
}

impl Panels for MovePanel {
    const ALL: &'static [Self] = &[
        Self::To,
        Self::Respellings,
        Self::Structural,
        Self::Notices,
        Self::Detail,
    ];
}

impl MovePanel {
    fn slot(self) -> Option<usize> {
        match self {
            Self::Respellings => Some(0),
            Self::Structural => Some(1),
            Self::Notices => Some(2),
            Self::To | Self::Detail => None,
        }
    }
}

/// What a move row points at, for the detail panel and the editor.
#[derive(Debug, Clone)]
pub enum MoveRow<'a> {
    Respelling(&'a Respelling),
    Structural(&'a FileChange),
    Notice(&'a Notice),
}

impl MoveRow<'_> {
    pub fn path(&self) -> &RelPath {
        match self {
            Self::Respelling(r) => &r.path,
            Self::Structural(f) => &f.path,
            Self::Notice(n) => &n.path,
        }
    }

    pub fn line(&self) -> u32 {
        match self {
            Self::Respelling(r) => r.start.line,
            Self::Structural(_) => 0,
            Self::Notice(n) => n.start.line,
        }
    }
}

impl MoveMode {
    pub fn new(from: RelPath, symbol: Option<String>) -> Self {
        let to = match &symbol {
            Some(_) => String::new(),
            None => from.short(),
        };
        Self {
            from,
            symbol,
            to,
            plan: None,
            error: None,
            focus: MovePanel::To,
            cursors: [Cursor::default(); 3],
            last_list: MovePanel::Respellings,
            detail_scroll: 0,
            preview: None,
            diff: false,
            busy: false,
        }
    }

    pub fn intent(&self) -> Option<Intent> {
        let to = self.to.trim();
        if to.is_empty() {
            return None;
        }
        Some(match &self.symbol {
            Some(name) => {
                Intent::MoveSymbol(vvv_engine::MoveSymbolIntent::new(name, &self.from, to))
            }
            None => Intent::Move(vvv_engine::MoveIntent::new(&self.from, to)),
        })
    }

    pub fn len(&self, panel: MovePanel) -> usize {
        let Some(plan) = &self.plan else {
            return 0;
        };
        match panel {
            MovePanel::Respellings => plan.respellings.len(),
            MovePanel::Structural => plan.structural.len(),
            MovePanel::Notices => plan.notices.len(),
            MovePanel::To | MovePanel::Detail => 0,
        }
    }

    pub fn cursor(&self, panel: MovePanel) -> Option<&Cursor> {
        panel.slot().map(|i| &self.cursors[i])
    }

    pub fn cursor_mut(&mut self, panel: MovePanel) -> Option<&mut Cursor> {
        panel.slot().map(move |i| &mut self.cursors[i])
    }

    pub fn list(&self) -> MovePanel {
        if self.focus.slot().is_some() {
            self.focus
        } else {
            self.last_list
        }
    }

    pub fn current(&self) -> Option<MoveRow<'_>> {
        let plan = self.plan.as_ref()?;
        let panel = self.list();
        let i = self.cursor(panel)?.index;
        Some(match panel {
            MovePanel::Respellings => MoveRow::Respelling(plan.respellings.get(i)?),
            MovePanel::Structural => MoveRow::Structural(&plan.files[*plan.structural.get(i)?]),
            MovePanel::Notices => MoveRow::Notice(plan.notices.get(i)?),
            MovePanel::To | MovePanel::Detail => return None,
        })
    }
}
