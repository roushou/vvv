use std::sync::OnceLock;

use super::{LineIndex, Position, Span};

/// Immutable file contents together with a line index, built the first time
/// a line is asked for: most files a search loads are never located in.
#[derive(Debug, Clone)]
pub struct SourceText {
    text: String,
    index: OnceLock<LineIndex>,
}

impl SourceText {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            index: OnceLock::new(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn slice(&self, span: Span) -> &str {
        &self.text[span.start..span.end]
    }

    pub fn position(&self, offset: usize) -> Position {
        self.line_index().position(&self.text, offset)
    }

    /// The byte offset of `position`, `None` past the end of the file.
    pub fn offset(&self, position: Position) -> Option<usize> {
        self.line_index().offset(&self.text, position)
    }

    /// The text of the zero-based `line`, without its terminator.
    pub fn line(&self, line: usize) -> Option<&str> {
        self.line_index().line_text(&self.text, line)
    }

    pub fn line_index(&self) -> &LineIndex {
        self.index.get_or_init(|| LineIndex::new(&self.text))
    }
}

impl From<String> for SourceText {
    fn from(text: String) -> Self {
        SourceText::new(text)
    }
}

impl From<&str> for SourceText {
    fn from(text: &str) -> Self {
        SourceText::new(text)
    }
}
