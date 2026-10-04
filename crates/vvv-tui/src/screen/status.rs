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
            .sections(m.focus(), |when| m.holds(when))
            .into_iter()
            .flat_map(|(_, rows)| rows)
            .filter_map(|row| row.legend.bar.map(|bar| (row, bar)))
            .map(|(row, bar)| {
                // An empty word means the meaning depends on the state.
                let what = if bar.word.is_empty() {
                    m.describe(row.binding.dispatch)
                } else {
                    bar.word.to_owned()
                };
                (
                    if row.partial {
                        row.labels
                    } else {
                        bar.keys.to_owned()
                    },
                    what,
                )
            })
            .fold(Vec::<(String, String)>::new(), |mut hints, hint| {
                if !hints.iter().any(|(_, what)| what == &hint.1) {
                    hints.push(hint);
                }
                hints
            })
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
        let budget = area.width.saturating_sub(9) as usize;
        let mut used = Line::from(spans.clone()).width();
        for (key, what) in self.hints() {
            let width = Line::from(format!("{key} {what}  ")).width();
            if used + width > budget {
                break;
            }
            spans.push(Span::styled(key, t.key));
            spans.push(Span::styled(format!(" {what}  "), t.dim));
            used += width;
        }
        let help_width = 9.min(area.width);
        Paragraph::new(Line::from(spans)).render(
            Rect::new(area.x, area.y, area.width - help_width, area.height),
            buf,
        );
        Paragraph::new(Line::from(vec![
            Span::styled(" f1", t.key),
            Span::styled(" help ", t.dim),
        ]))
        .render(
            Rect::new(area.right() - help_width, area.y, help_width, area.height),
            buf,
        );
    }
}
