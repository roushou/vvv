//! The renderer: a theme plus the drawing questions a screen asks of it.

use std::ops::Deref;

use ratatui::style::Style;
use ratatui::text::{Line as TextLine, Span};
use vvv_engine::protocol::display::Line;

use super::Theme;
use crate::model::FilePreview;

/// The number gutter every source line carries.
const GUTTER: usize = 8;

/// Drawing with one theme. A screen is handed a `Painter` and asks it for the
/// spans, lines and boxes a frame is made of, so the drawing questions have
/// one receiver. The colour policy stays on [`Theme`] and is reachable
/// directly: `painter.border(..)`, `painter.mark(..)`, `painter.hit`.
#[derive(Debug, Clone, Copy)]
pub struct Painter {
    theme: Theme,
}

impl Painter {
    pub const fn new(theme: Theme) -> Self {
        Self { theme }
    }

    pub fn plain() -> Self {
        Self::new(Theme::plain())
    }

    pub fn colored() -> Self {
        Self::new(Theme::colored())
    }

    /// The cursor mark after an input's text, when it takes keys.
    pub fn caret(&self, focused: bool) -> Span<'static> {
        Span::styled(if focused { "▏" } else { "" }, self.dim)
    }

    /// A [`display::Line`] as ratatui spans: each piece in its role's colour.
    pub fn spans(&self, line: &Line) -> Vec<Span<'static>> {
        line.pieces()
            .iter()
            .map(|p| Span::styled(p.text.clone(), self.role(p.role)))
            .collect()
    }

    /// A [`display::Line`] as a ratatui line.
    pub fn line(&self, line: &Line) -> TextLine<'static> {
        TextLine::from(self.spans(line))
    }

    /// Lines `first..first + height` of a file, `hit` (a byte range)
    /// highlighted, `marked` lines' numbers in the hit colour.
    pub fn source_window(
        &self,
        preview: &FilePreview,
        first: usize,
        height: usize,
        width: usize,
        hit: Option<(usize, usize)>,
        marked: Option<(usize, usize)>,
    ) -> Vec<TextLine<'static>> {
        (first..preview.line_count().min(first + height))
            .map(|n| self.source_line(preview, n, width, hit, marked))
            .collect()
    }

    /// One source line: number gutter, syntax colours, `hit` bytes and
    /// `marked` line emphasis.
    fn source_line(
        &self,
        preview: &FilePreview,
        n: usize,
        width: usize,
        hit: Option<(usize, usize)>,
        marked: Option<(usize, usize)>,
    ) -> TextLine<'static> {
        let width = width.saturating_sub(GUTTER);
        let in_mark = marked.is_some_and(|(a, b)| n >= a && n <= b);
        let number = Span::styled(
            format!("{:>5} │ ", n + 1),
            if in_mark { self.hit } else { self.dim },
        );
        let mut spans = vec![number];
        spans.extend(self.line_spans(preview, n, hit, width));
        TextLine::from(spans)
    }

    /// One source line as styled spans: syntax colours, then the hit's own
    /// bytes in the hit style on top.
    fn line_spans(
        &self,
        preview: &FilePreview,
        n: usize,
        hit: Option<(usize, usize)>,
        width: usize,
    ) -> Vec<Span<'static>> {
        let Some((start, end)) = preview.line_span(n) else {
            return Vec::new();
        };
        let text = preview.text();
        // Cut points: every highlight boundary and hit boundary inside the line.
        let mut cuts: Vec<usize> = vec![start, end];
        for h in &preview.highlights {
            if h.span.end > start && h.span.start < end {
                cuts.push(h.span.start.clamp(start, end));
                cuts.push(h.span.end.clamp(start, end));
            }
        }
        if let Some((hs, he)) = hit {
            cuts.push(hs.clamp(start, end));
            cuts.push(he.clamp(start, end));
        }
        cuts.sort_unstable();
        cuts.dedup();

        let mut spans = Vec::new();
        let mut used = 0usize;
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a == b {
                continue;
            }
            let piece = &text[a..b];
            let mut style = preview
                .highlights
                .iter()
                .find(|h| h.span.start <= a && b <= h.span.end)
                .map_or(Style::new(), |h| self.highlight(h.kind));
            if hit.is_some_and(|(hs, he)| hs <= a && b <= he) {
                style = style.patch(self.hit);
            }
            let count = piece.chars().count();
            if used + count > width {
                let keep = width.saturating_sub(used).saturating_sub(1);
                let cut: String = piece.chars().take(keep).collect();
                spans.push(Span::styled(format!("{cut}…"), style));
                break;
            }
            used += count;
            spans.push(Span::styled(piece.to_owned(), style));
        }
        spans
    }
}

impl Deref for Painter {
    type Target = Theme;

    fn deref(&self) -> &Theme {
        &self.theme
    }
}
