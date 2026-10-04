//! Inspection of retained source; offsets always refer to the displayed bytes.
use crate::model::FilePreview;
use std::sync::Arc;
use vvv_engine::{ContentId, RelPath, Span};

#[derive(Debug, Clone, Default)]
pub struct Inspection {
    pub term: String,
    pub hits: Arc<Vec<Span>>,
    pub cursor: Option<usize>,
    pub horizontal: usize,
    pub viewport: Option<usize>,
    pub line: Option<usize>,
    pub edit: Option<InspectionEdit>,
    identity: Option<(RelPath, ContentId, Span)>,
    columns: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionKind {
    Find,
    Line,
}

#[derive(Debug, Clone)]
pub struct InspectionEdit {
    pub kind: InspectionKind,
    pub text: String,
    pub caret: crate::input::Caret,
    pub error: Option<String>,
    term: String,
    cursor: Option<usize>,
    scroll: usize,
    horizontal: usize,
    line: Option<usize>,
}

impl Inspection {
    pub fn sync(&mut self, preview: &FilePreview, range: Span) {
        let identity = (preview.path.clone(), preview.content_id().clone(), range);
        if self.identity.as_ref() == Some(&identity) {
            return;
        }
        let same_site = self
            .identity
            .as_ref()
            .is_some_and(|(path, _, span)| path == &preview.path && *span == range);
        if !same_site {
            self.horizontal = 0;
            self.cursor = None;
            self.line = None;
        }
        self.edit = None;
        self.identity = Some(identity);
        self.columns = preview
            .text()
            .get(range.start..range.end)
            .unwrap_or("")
            .lines()
            .map(Self::columns)
            .max()
            .unwrap_or(0);
        self.horizontal = self.horizontal.min(self.columns.saturating_sub(1));
        self.rebuild(preview, range);
        self.line = self.line.filter(|n| {
            preview
                .lines_in(range)
                .is_some_and(|lines| lines.contains(n))
        });
    }

    /// Display columns, with four-column tab stops and Unicode cell widths.
    pub fn columns(text: &str) -> usize {
        text.chars().fold(0, |column, c| {
            column
                + if c == '\t' {
                    4 - column % 4
                } else {
                    ratatui::text::Span::raw(c.to_string()).width()
                }
        })
    }

    fn rebuild(&mut self, preview: &FilePreview, range: Span) {
        self.hits = Arc::new(if self.term.is_empty() {
            Vec::new()
        } else {
            preview
                .text()
                .get(range.start..range.end)
                .unwrap_or("")
                .match_indices(&self.term)
                .map(|(i, text)| Span::new(range.start + i, range.start + i + text.len()))
                .collect()
        });
        self.cursor = self.cursor.filter(|&i| i < self.hits.len());
    }

    pub fn begin(&mut self, kind: InspectionKind, scroll: usize) {
        self.edit = Some(InspectionEdit {
            kind,
            text: if kind == InspectionKind::Find {
                self.term.clone()
            } else {
                String::new()
            },
            caret: Default::default(),
            error: None,
            term: self.term.clone(),
            cursor: self.cursor,
            scroll,
            horizontal: self.horizontal,
            line: self.line,
        });
    }

    pub fn input(
        &mut self,
        character: Option<char>,
        clear: bool,
        preview: &FilePreview,
        range: Span,
    ) -> Option<usize> {
        let text = character.map(|c| c.to_string());
        let edit = if clear {
            crate::input::Edit::Clear
        } else if let Some(text) = &text {
            crate::input::Edit::Insert(text)
        } else {
            crate::input::Edit::Command(crate::input::EditCommand::Backspace)
        };
        self.edit_input(edit, preview, range)
    }
    pub fn edit_input(
        &mut self,
        input: crate::input::Edit<'_>,
        preview: &FilePreview,
        range: Span,
    ) -> Option<usize> {
        let edit = self.edit.as_mut()?;
        if !crate::input::TextInput::new(&mut edit.text, &mut edit.caret).apply(input) {
            return None;
        }
        edit.error = None;
        if edit.kind != InspectionKind::Find {
            return None;
        }
        self.term = edit.text.clone();
        let scroll = edit.scroll;
        self.cursor = None;
        self.rebuild(preview, range);
        let byte = preview
            .line_span(scroll)
            .map_or(range.start, |(start, _)| start);
        self.cursor = (!self.hits.is_empty())
            .then(|| self.hits.partition_point(|h| h.start < byte) % self.hits.len());
        self.reveal(preview)
    }

    pub fn step(&mut self, by: i32, preview: &FilePreview, scroll: usize) -> Option<usize> {
        if self.hits.is_empty() {
            return None;
        }
        let next = if let Some(index) = self.cursor {
            (index as i64 + i64::from(by)).rem_euclid(self.hits.len() as i64) as usize
        } else {
            let byte = preview.line_span(scroll).map_or(0, |(start, _)| start);
            let next = self.hits.partition_point(|h| h.start < byte) % self.hits.len();
            if by < 0 {
                (next + self.hits.len() - 1) % self.hits.len()
            } else {
                next
            }
        };
        self.cursor = Some(next);
        self.reveal(preview)
    }

    fn reveal(&mut self, preview: &FilePreview) -> Option<usize> {
        let hit = self.hits.get(self.cursor?)?;
        let line = preview.lines_in(*hit)?.start;
        let (mut start, _) = preview.line_span(line)?;
        if let Some((_, _, range)) = &self.identity
            && let Some(first) = preview.lines_in(*range).map(|r| r.start)
            && let Some((begin, _)) = preview.line_span(first)
            && let Some(indent) = preview.text().get(begin..range.start)
            && indent.chars().all(|c| matches!(c, ' ' | '\t'))
            && preview.text()[start..].starts_with(indent)
        {
            start += indent.len();
        }

        // A literal whitespace hit can start inside the removed outer indent.
        start = start.min(hit.start);
        let column = Self::columns(&preview.text()[start..hit.start]);
        let end = Self::columns(&preview.text()[start..hit.end]);
        let width = self.viewport.unwrap_or(80).max(1);
        if column < self.horizontal || end >= self.horizontal.saturating_add(width) {
            self.horizontal = column.saturating_sub(8.min(width / 4));
        }
        self.line = Some(line);
        Some(line)
    }

    pub fn accept(&mut self, lines: std::ops::Range<usize>) -> Option<usize> {
        let edit = self.edit.as_mut()?;
        if edit.kind == InspectionKind::Find {
            self.edit = None;
            return None;
        }
        let line = edit
            .text
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1));
        if let Some(line) = line.filter(|n| lines.contains(n)) {
            self.edit = None;
            self.line = Some(line);
            self.cursor = None;
            Some(line)
        } else {
            edit.error = Some(format!("line must be {}–{}", lines.start + 1, lines.end));
            None
        }
    }

    pub fn cancel(&mut self, preview: &FilePreview, range: Span) -> Option<usize> {
        let edit = self.edit.take()?;
        self.term = edit.term;
        self.cursor = edit.cursor;
        self.horizontal = edit.horizontal;
        self.line = edit.line;
        self.rebuild(preview, range);
        Some(edit.scroll)
    }

    pub fn horizontal_by(&mut self, by: i32) {
        self.horizontal = self
            .horizontal
            .saturating_add_signed(by as isize)
            .min(self.columns.saturating_sub(1));
    }

    pub fn active(&self) -> Option<Span> {
        self.cursor.and_then(|i| self.hits.get(i).copied())
    }

    pub fn retained_bytes(&self) -> usize {
        self.term.len()
            + self.hits.capacity() * std::mem::size_of::<Span>()
            + self
                .identity
                .as_ref()
                .map_or(0, |(path, _, _)| path.as_str().len())
            + 256
            + self
                .edit
                .as_ref()
                .map_or(0, |e| e.text.len() + e.term.len() + 128)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_ranges_are_exact_utf8_and_recomputed_for_new_displayed_content() {
        let file = |text: &str| {
            FilePreview::new(vvv_engine::File {
                path: "nested.rs".into(),
                text: text.into(),
                symbols: vec![],
                identifiers: vec![],
                highlights: vec![],
            })
        };
        let preview = file("outside é\n    fn nested() { é(); é(); }\noutside é");
        let start = preview.text().find("fn").unwrap();
        let end = preview.text().find("\noutside").unwrap();
        let range = Span::new(start, end);
        let mut inspection = Inspection::default();
        inspection.sync(&preview, range);
        inspection.begin(InspectionKind::Find, 1);
        assert_eq!(inspection.input(Some('é'), false, &preview, range), Some(1));
        assert_eq!(inspection.hits.len(), 2);
        assert!(inspection.hits.iter().all(|hit| range.contains(hit)));
        assert_eq!(inspection.step(1, &preview, 1), Some(1));
        assert_eq!(inspection.cursor, Some(1));
        inspection.step(1, &preview, 1);
        assert_eq!(inspection.cursor, Some(0));
        let refreshed = file("outside é\n    fn nested() { x(); x(); }\noutside é");
        inspection.sync(&refreshed, range);
        assert!(inspection.hits.is_empty());
        assert!(inspection.active().is_none());
    }
    #[test]
    fn find_reveals_long_line_hits_only_when_outside_the_visible_columns() {
        let text = format!("{}needle", "a".repeat(50));
        let preview = FilePreview::new(vvv_engine::File {
            path: "long.rs".into(),
            text,
            symbols: vec![],
            identifiers: vec![],
            highlights: vec![],
        });
        let range = Span::new(0, preview.text().len());
        let mut inspection = Inspection::default();
        inspection.sync(&preview, range);
        inspection.viewport = Some(20);
        inspection.begin(InspectionKind::Find, 0);
        for c in "needle".chars() {
            inspection.input(Some(c), false, &preview, range);
        }
        assert_eq!(inspection.horizontal, 45);
        inspection.step(1, &preview, 0);
        assert_eq!(inspection.horizontal, 45);
    }
    #[test]
    fn whitespace_find_in_dedented_declaration_keeps_valid_source_offsets() {
        let preview = FilePreview::new(vvv_engine::File {
            path: "nested.rs".into(),
            text: "    fn nested() {\n        inner();\n    }".into(),
            symbols: vec![],
            identifiers: vec![],
            highlights: vec![],
        });
        let range = Span::new(4, preview.text().len());
        let mut inspection = Inspection::default();
        inspection.sync(&preview, range);
        inspection.begin(InspectionKind::Find, 1);
        assert_eq!(inspection.input(Some(' '), false, &preview, range), Some(1));
        assert!(inspection.active().is_some());
        assert_eq!(inspection.horizontal, 0);
    }
}
