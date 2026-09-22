//! The title card of a screen: what it is, its counts on the right, and up
//! to two more lines beneath.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};

use super::{Painter, Region};

pub struct Header<'a> {
    painter: Painter,
    focused: bool,
    left: Line<'a>,
    right: Line<'a>,
    lines: Vec<Line<'a>>,
}

impl<'a> Header<'a> {
    pub fn new(painter: Painter, focused: bool, left: Line<'a>) -> Self {
        Self {
            painter,
            focused,
            left,
            right: Line::default(),
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

    /// Rows the header takes: its lines plus the border.
    pub fn height(&self) -> u16 {
        self.lines.len().max(1) as u16 + 2
    }

    /// Split `area` into the header's rows and the rest.
    pub fn areas(&self, area: Region) -> (Region, Region) {
        area.split(self.height())
    }
}

impl Widget for Header<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut right = self.right;
        if !right.spans.is_empty() {
            right.spans.insert(0, Span::raw(" "));
            right.spans.push(Span::raw(" "));
        }
        let block = Block::bordered()
            .border_style(self.painter.border(self.focused))
            .title(self.left)
            .title_top(right.right_aligned());
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(self.lines).render(inner, buf);
    }
}
