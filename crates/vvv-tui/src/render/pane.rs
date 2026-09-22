//! The one list widget every mode is made of: a border whose title is the
//! legend and the count, rows of one kind, a cursor kept in view. An empty
//! panel collapses to its title line.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use super::Painter;

pub struct Pane<'a> {
    painter: Painter,
    title: Line<'a>,
    focused: bool,
    /// Draw the cursor row in the cursor style even without the focus: a
    /// list whose selection stays live while an input has the keys.
    emphasized: bool,
    rows: Vec<Line<'a>>,
    cursor: Option<usize>,
    /// Shown when there are no rows and there is room.
    empty: &'a str,
    /// Lines to skip before the first row shown (text panels).
    scroll: usize,
}

impl<'a> Pane<'a> {
    pub fn new(painter: Painter, title: Line<'a>, focused: bool) -> Self {
        Self {
            painter,
            title,
            focused,
            emphasized: focused,
            rows: Vec::new(),
            cursor: None,
            empty: "∅",
            scroll: 0,
        }
    }

    /// Keep the cursor row visible while this pane does not have the focus.
    pub fn emphasized(mut self, emphasized: bool) -> Self {
        self.emphasized = emphasized;
        self
    }

    pub fn rows(mut self, rows: Vec<Line<'a>>) -> Self {
        self.rows = rows;
        self
    }

    pub fn cursor(mut self, cursor: Option<usize>) -> Self {
        self.cursor = cursor;
        self
    }

    pub fn empty(mut self, hint: &'a str) -> Self {
        self.empty = hint;
        self
    }

    pub fn scroll(mut self, scroll: usize) -> Self {
        self.scroll = scroll;
        self
    }
}

impl Widget for Pane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let t = self.painter;
        let mut title = self.title;
        title.spans.insert(0, Span::raw(" "));
        title.spans.push(Span::raw(" "));
        if area.height <= 1 {
            // Collapsed: the legend as a rule.
            Block::new()
                .borders(Borders::TOP)
                .border_style(t.border(self.focused))
                .title(title)
                .render(area, buf);
            return;
        }
        let block = Block::bordered()
            .border_style(t.border(self.focused))
            .title(title);
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.height == 0 {
            return;
        }
        if self.rows.is_empty() {
            Paragraph::new(Line::from(Span::styled(format!(" {}", self.empty), t.dim)))
                .render(inner, buf);
            return;
        }
        let height = inner.height as usize;
        let offset = match self.cursor {
            Some(cursor) => cursor.saturating_sub(height.saturating_sub(1)),
            None => self.scroll.min(self.rows.len().saturating_sub(1)),
        };
        let lines: Vec<Line> = self
            .rows
            .into_iter()
            .enumerate()
            .skip(offset)
            .take(height)
            .map(|(i, line)| {
                if Some(i) == self.cursor {
                    line.style(if self.emphasized { t.cursor } else { t.dim })
                } else {
                    line
                }
            })
            .collect();
        Paragraph::new(lines).render(inner, buf);
    }
}
