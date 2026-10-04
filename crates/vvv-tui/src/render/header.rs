//! The title card of a screen: what it is, its counts on the right, and up
//! to two more lines beneath.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};

use super::{Fit, Painter, Region};

pub struct Header<'a> {
    painter: Painter,
    focused: bool,
    left: Line<'a>,
    right: Line<'a>,
    bottom: Line<'a>,
    lines: Vec<Line<'a>>,
    input: Option<(Line<'a>, String, crate::input::Caret, String)>,
}

impl<'a> Header<'a> {
    pub fn new(painter: Painter, focused: bool, left: Line<'a>) -> Self {
        Self {
            painter,
            focused,
            left,
            right: Line::default(),
            bottom: Line::default(),
            lines: Vec::new(),
            input: None,
        }
    }

    pub fn right(mut self, right: Line<'a>) -> Self {
        self.right = right;
        self
    }

    pub fn input(
        mut self,
        prefix: Line<'a>,
        text: &str,
        caret: &crate::input::Caret,
        placeholder: &str,
    ) -> Self {
        self.input = Some((
            prefix,
            text.to_owned(),
            caret.clone(),
            placeholder.to_owned(),
        ));
        self
    }
    pub fn bottom(mut self, bottom: Line<'a>) -> Self {
        self.bottom = bottom;
        self
    }

    /// Rows the header takes: its lines plus the border.
    pub fn height(&self) -> u16 {
        self.lines.len() as u16 + 2 + u16::from(self.input.is_some())
    }

    /// Split `area` into the header's rows and the rest.
    pub fn areas(&self, area: Region) -> (Region, Region) {
        let width = area.rect().width.saturating_sub(4) as usize;
        let extra = if self.bottom.width() > width {
            self.bottom_text_rows(width).len() as u16
        } else {
            0
        };
        area.split(self.height() + extra)
    }

    fn bottom_text_rows(&self, width: usize) -> Vec<String> {
        let text = self.bottom.to_string();
        let mut rows = Vec::new();
        let mut row = String::new();
        for part in text.split_inclusive(" · ") {
            if !row.is_empty() && Line::from(format!("{row}{part}")).width() > width {
                rows.push(std::mem::take(&mut row));
            }
            if Line::from(part).width() > width {
                let mut wrapped = Fit(part, width).wrapped();
                row = wrapped.pop().unwrap_or_default();
                rows.extend(wrapped);
            } else {
                row.push_str(part);
            }
        }
        if !row.is_empty() {
            rows.push(row);
        }
        rows
    }

    /// Keep restriction emphasis when complete metadata needs multiple rows.
    fn wrapped_bottom(&self, width: usize) -> Vec<Line<'static>> {
        let mut styled = self.bottom.spans.iter().flat_map(|span| {
            span.content
                .chars()
                .map(move |c| (c, self.bottom.style.patch(span.style)))
        });
        self.bottom_text_rows(width)
            .into_iter()
            .map(|row| {
                let mut spans: Vec<Span<'static>> = Vec::new();
                for _ in row.chars() {
                    if let Some((c, style)) = styled.next() {
                        if let Some(last) = spans.last_mut().filter(|s| s.style == style) {
                            last.content.to_mut().push(c);
                        } else {
                            spans.push(Span::styled(c.to_string(), style));
                        }
                    }
                }
                Line::from(spans)
            })
            .collect()
    }
}

impl Widget for Header<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = area.width.saturating_sub(4) as usize;
        let wrapped = (self.bottom.width() > width).then(|| self.wrapped_bottom(width));
        let mut right = self.right;
        if !right.spans.is_empty() {
            right.spans.insert(0, Span::raw(" "));
            right.spans.push(Span::raw(" "));
        }
        let mut lines = self.lines;
        if let Some((mut prefix, text, caret, placeholder)) = self.input {
            if prefix.width() > width / 3 {
                prefix = Line::from(Span::styled(
                    Fit(&prefix.to_string(), width / 3).to_string(),
                    self.painter.dim,
                ));
            }
            let available = width.saturating_sub(prefix.width());
            prefix.spans.extend(
                caret
                    .line(&text, self.painter, self.focused, available)
                    .spans,
            );
            if text.is_empty() {
                prefix.spans.push(Span::styled(
                    Fit(
                        &placeholder,
                        available.saturating_sub(usize::from(self.focused)),
                    )
                    .to_string(),
                    self.painter.dim,
                ));
            }
            lines.push(prefix);
        }

        let bottom = if let Some(wrapped) = wrapped {
            lines.extend(wrapped);
            Line::default()
        } else {
            self.bottom
        };
        let block = Block::bordered()
            .border_style(self.painter.border(self.focused))
            .title(self.left)
            .title_bottom(bottom)
            .title_top(right.right_aligned());
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(lines).render(inner, buf);
    }
}
