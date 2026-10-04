//! The one list widget every mode is made of: a border whose title is the
//! legend and the count, rows of one kind, a cursor kept in view. An empty
//! panel collapses to its title line.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use super::{Fit, Painter};

pub struct Pane<'a> {
    painter: Painter,
    title: Line<'a>,
    right: Line<'a>,
    footer: Line<'a>,
    location: Option<String>,
    focused: bool,
    /// Draw the cursor row in the cursor style even without the focus: a
    /// list whose selection stays live while an input has the keys.
    emphasized: bool,
    rows: Vec<Line<'a>>,
    prefix: Vec<Line<'a>>,
    cursor: Option<usize>,
    /// Shown when there are no rows and there is room.
    empty: &'a str,
    /// Lines to skip before the first row shown (text panels).
    scroll: usize,
    list_offset: Option<usize>,
}

impl<'a> Pane<'a> {
    pub fn new(painter: Painter, title: Line<'a>, focused: bool) -> Self {
        Self {
            painter,
            title,
            right: Line::default(),
            footer: Line::default(),
            location: None,
            focused,
            emphasized: focused,
            rows: Vec::new(),
            prefix: Vec::new(),
            cursor: None,
            empty: "∅",
            scroll: 0,
            list_offset: None,
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
    /// Fixed rows above the scrollable list, such as a live file filter.
    pub fn prefix(mut self, prefix: Vec<Line<'a>>) -> Self {
        self.prefix = prefix;
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

    /// Explicit list viewport; selection highlighting does not force scrolling.
    pub fn list_offset(mut self, offset: usize) -> Self {
        self.list_offset = Some(offset);
        self
    }

    pub fn right(mut self, right: Line<'a>) -> Self {
        self.right = right;
        self
    }
    pub fn footer(mut self, footer: Line<'a>) -> Self {
        self.footer = footer;
        self
    }
    pub fn location(mut self, location: String) -> Self {
        self.location = Some(location);
        self
    }

    pub fn content_height(&self, area: Rect) -> usize {
        let height = area.height.saturating_sub(2) as usize;
        let width = area.width.saturating_sub(4) as usize;
        let extra = self
            .location
            .as_ref()
            .filter(|p| self.title.width() + 3 + Line::from(p.as_str()).width() > width)
            .map_or(0, |p| Fit(p, width).wrapped().len());
        height.saturating_sub((extra + self.prefix.len()).min(height.saturating_sub(1)))
    }
}

impl Widget for Pane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let t = self.painter;
        let budget = area.width.saturating_sub(4) as usize;
        let mut title = self.title;
        let mut metadata = Vec::new();
        if let Some(location) = &self.location {
            if title.width() + 3 + Line::from(location.as_str()).width() <= budget {
                title.spans.push(Span::styled(" · ", t.dim));
                title.spans.extend(t.path_line(location).spans);
            } else {
                metadata = Fit(location, budget)
                    .wrapped()
                    .into_iter()
                    .map(|row| {
                        let mut line = t.path_line(&row);
                        line.spans.insert(0, Span::raw(" "));
                        line
                    })
                    .collect();
            }
        }
        metadata.extend(self.prefix);
        let title_width = title.width();
        title.spans.insert(0, Span::raw(" "));
        title.spans.push(Span::raw(" "));
        if area.height <= 1 {
            // Collapsed: the legend as a rule.
            let mut block = Block::new()
                .borders(Borders::TOP)
                .border_style(t.border(self.focused))
                .title(title);
            let mut right = self.right;
            if right.width() > 0 && area.width as usize > right.width() + title_width + 6 {
                right.spans.insert(0, Span::raw(" "));
                right.spans.push(Span::raw(" "));
                block = block.title_top(right.right_aligned());
            }
            block.render(area, buf);
            return;
        }
        let mut block = Block::bordered()
            .border_style(t.border(self.focused))
            .title(title);
        let mut right = self.right;
        if right.width() > 0 && area.width as usize > right.width() + title_width + 6 {
            right.spans.insert(0, Span::raw(" "));
            right.spans.push(Span::raw(" "));
            block = block.title_top(right.right_aligned());
        }
        if !self.footer.spans.is_empty() {
            block = block.title_bottom(self.footer);
        }
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.height == 0 {
            return;
        }
        let metadata_height = metadata.len().min(inner.height.saturating_sub(1) as usize) as u16;
        Paragraph::new(metadata).render(
            Rect::new(inner.x, inner.y, inner.width, metadata_height),
            buf,
        );
        let inner = Rect::new(
            inner.x,
            inner.y + metadata_height,
            inner.width,
            inner.height - metadata_height,
        );
        if self.rows.is_empty() {
            Paragraph::new(Line::from(Span::styled(format!(" {}", self.empty), t.dim)))
                .render(inner, buf);
            return;
        }
        let height = inner.height as usize;
        let offset = self.list_offset.unwrap_or_else(|| match self.cursor {
            Some(cursor) => cursor.saturating_sub(height.saturating_sub(1)),
            None => self.scroll.min(self.rows.len().saturating_sub(1)),
        });
        let lines = self
            .rows
            .into_iter()
            .enumerate()
            .skip(offset)
            .take(height)
            .map(|(i, line)| {
                if Some(i) == self.cursor {
                    t.selected_line(line, self.emphasized, inner.width as usize)
                } else {
                    line
                }
            })
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buf);
    }
}
