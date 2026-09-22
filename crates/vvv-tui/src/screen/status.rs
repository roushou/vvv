//! Bottom line: the status message, a busy mark, and the keys that matter
//! in the focused panel — `⏎` always spelled out.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::model::{Level, Model};
use crate::render::Painter;

pub struct StatusBar<'a> {
    model: &'a Model,
    painter: Painter,
}

impl<'a> StatusBar<'a> {
    pub fn new(model: &'a Model, painter: Painter) -> Self {
        Self { model, painter }
    }

    /// (keys, what) for where the focus is: the active layers' entries
    /// that ask to be shown, most specific first.
    fn hints(&self) -> Vec<(String, String)> {
        let m = self.model;
        m.screen()
            .rows(m.focus())
            .into_iter()
            .filter_map(|row| row.legend.bar.map(|bar| (row, bar)))
            .map(|(row, bar)| {
                // An empty word means the meaning depends on the state.
                let what = if bar.word.is_empty() {
                    m.describe(row.binding.dispatch)
                } else {
                    bar.word.to_owned()
                };
                (bar.keys.to_owned(), what)
            })
            .collect()
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let t = self.painter;
        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::styled(
            format!(" {} ", self.model.mode.name()),
            t.title,
        ));
        if let Some((level, message)) = &self.model.status.message {
            let style = match level {
                Level::Info => t.dim,
                Level::Error => t.error,
            };
            spans.push(Span::styled(format!(" {message}  "), style));
        }
        if self.model.status.busy || self.model.arriving {
            spans.push(Span::styled("… ", t.dim));
        }
        for (key, what) in self.hints() {
            spans.push(Span::styled(key, t.key));
            spans.push(Span::styled(format!(" {what}  "), t.dim));
        }
        Paragraph::new(Line::from(spans)).render(area, buf);
    }
}
