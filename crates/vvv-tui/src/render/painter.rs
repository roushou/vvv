//! The renderer: a theme plus the drawing questions a screen asks of it.

use std::ops::Deref;

use ratatui::style::Style;
use ratatui::text::{Line as TextLine, Span};
use vvv_engine::protocol::display::Line;

use super::Theme;
use crate::model::FilePreview;

/// The number gutter in file context views.
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

    /// Selection fills the row without dimming its text or replacing semantic spans.
    pub fn selected_line<'a>(
        &self,
        mut line: TextLine<'a>,
        emphasized: bool,
        width: usize,
    ) -> TextLine<'a> {
        let padding = width.saturating_sub(line.width());
        if padding > 0 {
            line.spans.push(Span::raw(" ".repeat(padding)));
        }
        line.patch_style(self.selection(emphasized))
    }

    /// A continuous path: readable directories with the filename emphasized
    /// in place. Wrapped path segments retain their original reading order.
    pub fn path_line(&self, path: &str) -> TextLine<'static> {
        let split = path.rfind('/').map_or(0, |i| i + 1);
        TextLine::from(vec![
            Span::raw(path[..split].to_owned()),
            Span::styled(path[split..].to_owned(), self.title),
        ])
    }

    /// Fit source around its hit, retaining the beginning of the statement
    /// when a middle section must be omitted. Ellipses mark every omission;
    /// budgets use terminal columns, including wide Unicode characters.
    pub fn excerpt(&self, line: &Line, width: usize) -> Vec<Span<'static>> {
        use vvv_engine::protocol::display::Role;

        if width == 0 {
            return Vec::new();
        }
        let chars: Vec<_> = line
            .pieces()
            .iter()
            .flat_map(|p| p.text.chars().map(move |c| (c, p.role)))
            .map(|(c, role)| (c, role, Span::raw(c.to_string()).width()))
            .collect();
        let columns =
            |start: usize, end: usize| chars[start..end].iter().map(|c| c.2).sum::<usize>();
        if columns(0, chars.len()) <= width {
            return self.spans(line);
        }
        if width == 1 {
            return vec![Span::styled("…", self.dim)];
        }
        let prefix = |start: usize, end: usize, budget: usize| {
            let mut used = 0;
            start
                + chars[start..end]
                    .iter()
                    .take_while(|c| {
                        used += c.2;
                        used <= budget
                    })
                    .count()
        };
        let suffix = |start: usize, end: usize, budget: usize| {
            let mut used = 0;
            end - chars[start..end]
                .iter()
                .rev()
                .take_while(|c| {
                    used += c.2;
                    used <= budget
                })
                .count()
        };
        let hit = chars
            .iter()
            .position(|c| c.1 == Role::Hit)
            .zip(chars.iter().rposition(|c| c.1 == Role::Hit).map(|i| i + 1));
        let ranges = match hit {
            Some((start, end))
                if columns(0, end) + columns(end, chars.len()).min(8) + 1 > width =>
            {
                let hit_width = columns(start, end);
                if hit_width + 2 >= width {
                    let leading = usize::from(start > 0);
                    std::iter::once(start..prefix(start, end, width.saturating_sub(leading + 1)))
                        .collect::<Vec<_>>()
                } else {
                    let after_budget = columns(end, chars.len())
                        .min((width - hit_width - 2) / 3)
                        .min(12);
                    let before_budget = width - hit_width - after_budget - 2;
                    let word = |c: char| c.is_alphanumeric() || c == '_';
                    let mut head = prefix(0, start, before_budget * 2 / 3);
                    if head > 0 && head < start && word(chars[head - 1].0) && word(chars[head].0) {
                        while head > 0 && word(chars[head - 1].0) {
                            head -= 1;
                        }
                    }
                    let mut near = suffix(head, start, before_budget - columns(0, head));
                    if near > head && near < start && word(chars[near - 1].0) && word(chars[near].0)
                    {
                        while near < start && word(chars[near].0) {
                            near += 1;
                        }
                    }
                    let tail = prefix(end, chars.len(), after_budget);
                    if head == near {
                        std::iter::once(0..tail).collect()
                    } else {
                        vec![0..head, near..tail]
                    }
                }
            }
            _ => std::iter::once(0..prefix(0, chars.len(), width.saturating_sub(1))).collect(),
        };
        let mut out: Vec<Span<'static>> = Vec::new();
        let mut previous = 0;
        for range in ranges {
            if range.start > previous {
                out.push(Span::styled("…", self.dim));
            }
            for &(c, role, _) in &chars[range.clone()] {
                let style = self.role(role);
                if let Some(span) = out.last_mut().filter(|s| s.style == style) {
                    span.content.to_mut().push(c);
                } else {
                    out.push(Span::styled(c.to_string(), style));
                }
            }
            previous = range.end;
        }
        if previous < chars.len() {
            out.push(Span::styled("…", self.dim));
        }
        out
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

    /// A declaration without its enclosing indentation or a number gutter.
    /// The indentation comes from the declaration's first line, never the
    /// visible window, so scrolling preserves the body's relative indentation.
    pub fn code_window(
        &self,
        preview: &FilePreview,
        declaration: vvv_engine::Span,
        visible: std::ops::Range<usize>,
        width: usize,
        hit: Option<vvv_engine::Span>,
    ) -> Vec<TextLine<'static>> {
        let Some(lines) = preview.lines_in(declaration) else {
            return Vec::new();
        };
        let Some((start, _)) = preview.line_span(lines.start) else {
            return Vec::new();
        };
        let prefix = &preview.text()[start..declaration.start];
        let indent = if prefix.chars().all(|c| matches!(c, ' ' | '\t')) {
            prefix
        } else {
            ""
        };
        (visible.start.max(lines.start)..lines.end.min(visible.end))
            .filter_map(|n| {
                let (mut start, end) = preview.line_span(n)?;
                if preview.text()[start..end].starts_with(indent) {
                    start += indent.len();
                }
                let range = (start.max(declaration.start), end.min(declaration.end));
                let mut spans = vec![Span::raw(" ")];
                spans.extend(self.source_spans(
                    preview,
                    range,
                    hit.map(|s| (s.start, s.end)),
                    width.saturating_sub(1),
                ));
                Some(TextLine::from(spans))
            })
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
        self.source_spans(preview, (start, end), hit, width)
    }

    /// Style original source coordinates even when a view clips indentation.
    fn source_spans(
        &self,
        preview: &FilePreview,
        (start, end): (usize, usize),
        hit: Option<(usize, usize)>,
        width: usize,
    ) -> Vec<Span<'static>> {
        if start >= end {
            return Vec::new();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use vvv_engine::{Highlight, HighlightKind, Span as SourceSpan};

    #[test]
    fn long_excerpts_keep_the_hit_and_statement_context_within_terminal_columns() {
        use vvv_engine::protocol::display::Role;
        let painter = Painter::colored();
        for (before, hit, after) in [
            (
                "use vvv_engine::{Answer, Call, ErrorCode, Failure, ",
                "Engine",
                ", Reply};",
            ),
            (
                "let 世界 = some_really_long_expression(",
                "Engine",
                "::new());",
            ),
            ("", "ExtremelyLongMatchedIdentifier", "::new();"),
        ] {
            let line = Line::new()
                .and(Role::Plain, before)
                .and(Role::Hit, hit)
                .and(Role::Plain, after);
            for width in 0..80 {
                let spans = painter.excerpt(&line, width);
                let rendered = TextLine::from(spans.clone());
                assert!(rendered.width() <= width, "width {width}: {rendered}");
                if width >= hit.len() + 4 {
                    assert!(
                        spans
                            .iter()
                            .any(|s| s.style == painter.hit && s.content == hit)
                    );
                }
            }
        }
        let line = Line::new()
            .and(
                Role::Plain,
                "use vvv_engine::{Answer, Call, ErrorCode, Failure, ",
            )
            .and(Role::Hit, "Engine")
            .and(Role::Plain, ", Reply};");
        let excerpt = TextLine::from(painter.excerpt(&line, 49)).to_string();
        assert!(excerpt.starts_with("use vvv_engine::{"), "{excerpt}");
        assert!(excerpt.contains("Engine"));
        assert!(excerpt.contains('…'));
    }

    #[test]
    fn declaration_clipping_keeps_original_highlight_coordinates() {
        let text = "mod outer { fn nested() {} fn neighbor() {} }";
        let start = text.find("fn nested").unwrap();
        let end = text.find(" fn neighbor").unwrap();
        let preview = FilePreview::new(vvv_engine::File {
            identifiers: vec![],
            path: "nested.rs".into(),
            text: text.into(),
            symbols: vec![],
            highlights: vec![Highlight {
                span: SourceSpan::new(start, start + 2),
                kind: HighlightKind::Keyword,
            }],
        });
        let painter = Painter::colored();
        let lines = painter.code_window(&preview, SourceSpan::new(start, end), 0..10, 80, None);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), " fn nested() {}");
        assert_eq!(lines[0].spans[1].content, "fn");
        assert_eq!(
            lines[0].spans[1].style,
            painter.highlight(HighlightKind::Keyword)
        );
    }
}
