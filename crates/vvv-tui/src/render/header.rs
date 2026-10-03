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
        }
    }

    pub fn right(mut self, right: Line<'a>) -> Self {
        self.right = right;
        self
    }

    pub fn line(mut self, line: Line<'a>) -> Self {
        self.lines.push(line);
        self
    }

    pub fn bottom(mut self, bottom: Line<'a>) -> Self {
        self.bottom = bottom;
        self
    }

    /// Rows the header takes: its lines plus the border.
    pub fn height(&self) -> u16 {
        self.lines.len() as u16 + 2
    }

    /// Split `area` into the header's rows and the rest.
    pub fn areas(&self, area: Region) -> (Region, Region) {
        let width = area.rect().width.saturating_sub(4) as usize;
        let extra = if self.bottom.width() > width {
            Fit(&self.bottom.to_string(), width).wrapped().len() as u16
        } else {
            0
        };
        area.split(self.height() + extra)
    }
}

impl Widget for Header<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut right = self.right;
        if !right.spans.is_empty() {
            right.spans.insert(0, Span::raw(" "));
            right.spans.push(Span::raw(" "));
        }
        let width = area.width.saturating_sub(4) as usize;
        let mut lines = self.lines;
        let bottom = if self.bottom.width() > width {
            lines.extend(
                Fit(&self.bottom.to_string(), width)
                    .wrapped()
                    .into_iter()
                    .map(Line::from),
            );
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
