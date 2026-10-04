//! A clipped source window with syntax, inspection hits and terminal columns.
use super::Painter;
use crate::model::FilePreview;
use ratatui::{
    style::Modifier,
    text::{Line, Span},
};
use std::ops::Range;
use vvv_engine::Span as SourceSpan;

pub struct CodeWindow<'a> {
    pub preview: &'a FilePreview,
    pub painter: Painter,
    pub visible: Range<usize>,
    pub width: usize,
    pub declaration: Option<SourceSpan>,
    pub origin: Option<SourceSpan>,
    pub marked: Option<(usize, usize)>,
    pub horizontal: usize,
    pub hits: &'a [SourceSpan],
    pub active: Option<SourceSpan>,
}

impl CodeWindow<'_> {
    pub fn rows(&self) -> Vec<Line<'static>> {
        let lines = self
            .declaration
            .and_then(|s| self.preview.lines_in(s))
            .unwrap_or(0..self.preview.line_count());
        let indent = self
            .declaration
            .and_then(|s| {
                let (start, _) = self.preview.line_span(lines.start)?;
                let prefix = self.preview.text().get(start..s.start)?;
                prefix
                    .chars()
                    .all(|c| matches!(c, ' ' | '\t'))
                    .then_some(prefix)
            })
            .unwrap_or("");
        (self.visible.start.max(lines.start)..self.visible.end.min(lines.end))
            .filter_map(|n| {
                let (mut start, mut end) = self.preview.line_span(n)?;
                let inset = if let Some(declaration) = self.declaration {
                    if self.preview.text()[start..end].starts_with(indent) {
                        start += indent.len();
                    }
                    start = start.max(declaration.start);
                    end = end.min(declaration.end);
                    Span::raw(if self.horizontal > 0 { "‹" } else { " " })
                } else {
                    let marked = self.marked.is_some_and(|(a, b)| n >= a && n <= b);
                    Span::styled(
                        format!(
                            "{:>5} │{}",
                            n + 1,
                            if self.horizontal > 0 { "‹" } else { " " }
                        ),
                        if marked {
                            self.painter.hit
                        } else {
                            self.painter.dim
                        },
                    )
                };
                let width = self.width.saturating_sub(inset.width());
                let mut spans = vec![inset];
                spans.extend(self.spans(start, end, width));
                Some(Line::from(spans))
            })
            .collect()
    }

    fn spans(&self, start: usize, end: usize, width: usize) -> Vec<Span<'static>> {
        if start >= end || width == 0 {
            return vec![];
        }
        let t = self.painter;
        let text = self.preview.text();
        let highlights = self.preview.highlights_in(start, end);
        let first = self.hits.partition_point(|s| s.end <= start);
        let last = self.hits.partition_point(|s| s.start < end);
        let hits = &self.hits[first..last];
        let mut cuts = vec![start, end];
        for span in highlights
            .iter()
            .map(|h| h.span)
            .chain(hits.iter().copied())
            .chain(self.origin)
            .chain(self.active)
        {
            cuts.push(span.start.clamp(start, end));
            cuts.push(span.end.clamp(start, end));
        }
        cuts.sort_unstable();
        cuts.dedup();
        let total = text[start..end].chars().fold(0, |col, c| {
            col + if c == '\t' {
                4 - col % 4
            } else {
                Span::raw(c.to_string()).width()
            }
        });
        let overflow = total > self.horizontal.saturating_add(width);
        let right = self
            .horizontal
            .saturating_add(width.saturating_sub(usize::from(overflow)));
        let mut column = 0;
        let mut spans: Vec<Span<'static>> = Vec::new();
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let mut style = highlights
                .iter()
                .find(|h| h.span.start <= a && b <= h.span.end)
                .map_or(Default::default(), |h| t.highlight(h.kind));
            if self.origin.is_some_and(|h| h.start <= a && b <= h.end) {
                style = style.patch(t.hit);
            }
            if hits
                .get(hits.partition_point(|h| h.end <= a))
                .is_some_and(|h| h.start <= a && b <= h.end)
            {
                style = style.patch(t.key).add_modifier(Modifier::UNDERLINED);
            }
            if self.active.is_some_and(|h| h.start <= a && b <= h.end) {
                style = style
                    .patch(t.selection(true))
                    .add_modifier(Modifier::BOLD)
                    .remove_modifier(Modifier::DIM);
            }
            for c in text[a..b].chars() {
                let cells = if c == '\t' {
                    4 - column % 4
                } else {
                    Span::raw(c.to_string()).width()
                };
                let next = column + cells;
                if next > self.horizontal && column < right {
                    let visible = next.min(right).saturating_sub(column.max(self.horizontal));
                    let rendered = if c == '\t' || visible < cells {
                        " ".repeat(visible)
                    } else {
                        c.to_string()
                    };
                    if let Some(last) = spans.last_mut().filter(|s| s.style == style) {
                        last.content.to_mut().push_str(&rendered);
                    } else {
                        spans.push(Span::styled(rendered, style));
                    }
                } else if cells == 0
                    && column > self.horizontal
                    && column <= right
                    && let Some(last) = spans.last_mut()
                {
                    last.content.to_mut().push(c);
                }
                column = next;
            }
        }
        if overflow {
            spans.push(Span::styled("…", t.dim));
        }
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vvv_engine::{File, Highlight, HighlightKind};

    #[test]
    fn horizontal_clipping_keeps_unicode_tabs_and_highlights_in_terminal_columns() {
        let text = "\t界界e\u{301}needle_tail";
        let hit = SourceSpan::new(
            text.find("needle").unwrap(),
            text.find("needle").unwrap() + 6,
        );
        let preview = FilePreview::new(File {
            path: "wide.rs".into(),
            text: text.into(),
            symbols: vec![],
            identifiers: vec![],
            highlights: vec![Highlight {
                span: SourceSpan::new(0, text.len()),
                kind: HighlightKind::String,
            }],
        });
        let painter = Painter::colored();
        for width in 1..28 {
            for horizontal in 0..28 {
                let rows = CodeWindow {
                    preview: &preview,
                    painter,
                    visible: 0..1,
                    width,
                    declaration: Some(SourceSpan::new(0, text.len())),
                    origin: None,
                    marked: None,
                    horizontal,
                    hits: &[hit],
                    active: Some(hit),
                }
                .rows();
                assert!(
                    rows[0].width() <= width,
                    "width {width}, offset {horizontal}: {:?}",
                    rows[0]
                );
                assert!(!rows[0].to_string().contains('\t'));
            }
        }
        let row = CodeWindow {
            preview: &preview,
            painter,
            visible: 0..1,
            width: 30,
            declaration: Some(SourceSpan::new(0, text.len())),
            origin: None,
            marked: None,
            horizontal: 9,
            hits: &[hit],
            active: Some(hit),
        }
        .rows()
        .remove(0);
        let found = row
            .spans
            .iter()
            .find(|s| s.content.contains("needle"))
            .unwrap();
        assert_eq!(found.style.bg, painter.cursor.bg);
        assert!(found.style.add_modifier.contains(Modifier::BOLD));
        assert!(
            row.spans
                .iter()
                .any(|s| s.content.contains("_tail") && s.style.fg == painter.string.fg)
        );
    }
}
