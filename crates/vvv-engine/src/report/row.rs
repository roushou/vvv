//! Display rows and their actionable source sites.
use crate::protocol::display::Line;
use vvv_core::RelPath;

/// A row: the line to draw, and the source it stands for.
#[derive(Debug, Clone)]
pub struct Row {
    pub line: Line,
    /// Where the row is, when an interface can act on it.
    pub source: Option<Source>,
}

/// The source a row stands for: the file and the line in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub path: RelPath,
    pub line: u32,
}

impl Row {
    /// A row no renderer can act on.
    pub fn new(line: Line) -> Self {
        Self { line, source: None }
    }

    /// A row that stands for a source.
    pub fn at(line: Line, path: RelPath, at: u32) -> Self {
        Self {
            line,
            source: Some(Source { path, line: at }),
        }
    }

    /// Wrap plain lines as rows no renderer can act on.
    pub fn wrap(lines: Vec<Line>) -> Vec<Self> {
        lines.into_iter().map(Self::new).collect()
    }
}
