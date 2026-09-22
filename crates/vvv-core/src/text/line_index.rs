use super::Position;

/// Maps byte offsets to [`Position`]s. Built once per file, queried many times.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset at which each line starts. Always begins with `0`.
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self { line_starts }
    }

    /// Zero-based line containing `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        self.line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1)
    }

    /// Byte offset where `line` starts.
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line).copied()
    }

    /// The text of `line` without its line terminator.
    pub fn line_text<'t>(&self, text: &'t str, line: usize) -> Option<&'t str> {
        let start = self.line_start(line)?;
        let end = self.line_start(line + 1).unwrap_or(text.len());
        Some(text[start..end].trim_end_matches(['\n', '\r']))
    }

    /// The byte offset of `position` in `text`; `None` past the end of the
    /// file. Columns count characters, as [`Self::position`] produces them.
    pub fn offset(&self, text: &str, position: Position) -> Option<usize> {
        let line_start = *self.line_starts.get(position.line as usize)?;
        let line = &text[line_start..];
        let line = line.split('\n').next().unwrap_or(line);
        let column = line
            .char_indices()
            .nth(position.column as usize)
            .map(|(i, _)| i)
            .or_else(|| (line.chars().count() == position.column as usize).then_some(line.len()))?;
        Some(line_start + column)
    }

    /// Resolve `offset` against `text` (the same text this index was built from).
    pub fn position(&self, text: &str, offset: usize) -> Position {
        let offset = offset.min(text.len());
        let line = self.line_of(offset);
        let line_start = self.line_starts[line];
        let column = text[line_start..offset].chars().count();
        Position::new(line as u32, column as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_chars_not_bytes() {
        let text = "héllo\nwörld";
        let index = LineIndex::new(text);
        assert_eq!(index.position(text, 0), Position::new(0, 0));
        assert_eq!(index.position(text, text.len()), Position::new(1, 5));
        let w = text.find('w').unwrap();
        assert_eq!(index.position(text, w + 3), Position::new(1, 2));
    }

    #[test]
    fn line_text_strips_terminators() {
        let text = "a\r\nbb\nccc";
        let index = LineIndex::new(text);
        assert_eq!(index.line_text(text, 0), Some("a"));
        assert_eq!(index.line_text(text, 1), Some("bb"));
        assert_eq!(index.line_text(text, 2), Some("ccc"));
        assert_eq!(index.line_text(text, 3), None);
    }

    #[test]
    fn offset_at_newline_belongs_to_the_line_it_ends() {
        let text = "a\nb";
        let index = LineIndex::new(text);
        assert_eq!(index.position(text, 1), Position::new(0, 1));
        assert_eq!(index.position(text, 2), Position::new(1, 0));
    }
}
