//! Compact review rows keep complete paths separate from source excerpts.

use super::{Fit, Painter, Pane};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
};
use vvv_engine::{
    Match, RelPath,
    protocol::{display, vocabulary::Mark},
};

pub struct ReviewItem<'a> {
    pub path: &'a RelPath,
    pub line: Option<u32>,
    pub mark: Mark,
    pub text: display::Line,
}

impl<'a> ReviewItem<'a> {
    pub fn matched(m: &'a Match, ticked: bool) -> Self {
        Self {
            path: &m.path,
            line: Some(m.start.line + 1),
            mark: if ticked { Mark::Ticked } else { Mark::Unticked },
            text: display::Line::hit(m, display::Role::Plain),
        }
    }
}

pub struct ReviewList<'a> {
    pub painter: Painter,
    pub items: Vec<ReviewItem<'a>>,
    pub numbered: bool,
    pub active: bool,
}

impl<'a> ReviewList<'a> {
    pub fn pane(self, area: Rect, title: Line<'a>, focused: bool, cursor: usize) -> Pane<'a> {
        let t = self.painter;
        let width = area.width.saturating_sub(2) as usize;
        let digits = self
            .items
            .iter()
            .filter_map(|item| item.line)
            .max()
            .unwrap_or(1)
            .to_string()
            .len();
        let mut lines = Vec::new();
        let mut current = None;
        let mut group_start = 0;
        let mut selected_group = 0;
        let mut selected = None;
        for (i, item) in self.items.iter().enumerate() {
            if current != Some(item.path) {
                group_start = lines.len();
                for row in Fit(item.path.as_str(), width.saturating_sub(2)).wrapped() {
                    let mut heading = t.path_line(&row);
                    heading.spans.insert(0, Span::raw("  "));
                    lines.push(heading);
                }
                current = Some(item.path);
            }
            if i == cursor {
                selected = Some(lines.len());
                selected_group = group_start;
            }
            let mut spans = vec![
                Span::styled(
                    if i == cursor && self.active {
                        "> "
                    } else {
                        "  "
                    },
                    t.selection_marker(focused),
                ),
                t.glyph(item.mark),
            ];
            if self.numbered {
                spans.push(Span::styled(format!("{}. ", i + 1), t.dim));
            }
            spans.push(Span::styled(
                item.line
                    .map_or_else(|| " ".repeat(digits + 1), |n| format!("{n:>digits$} ")),
                t.dim,
            ));
            let used: usize = spans.iter().map(Span::width).sum();
            spans.extend(t.excerpt(&item.text, width.saturating_sub(used)));
            lines.push(Line::from(spans));
        }
        let height = area.height.saturating_sub(2) as usize;
        let offset = selected.map_or(0, |row| {
            if lines.len() <= height || row < height {
                0
            } else if row.saturating_sub(selected_group) < height {
                selected_group
            } else {
                row.saturating_sub(height.saturating_sub(1))
            }
        });
        Pane::new(t, title, focused)
            .rows(lines)
            .cursor(selected.filter(|_| self.active))
            .list_offset(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, widgets::Widget};

    #[test]
    fn wrapped_groups_map_selection_to_the_item_and_keep_its_colors_when_focus_leaves() {
        let path = RelPath::from("crates/世界/src/long_directory/same.rs");
        let other = RelPath::from("crates/another/src/same.rs");
        let area = Rect::new(0, 0, 42, 16);
        let painter = Painter::colored();
        for focused in [true, false] {
            let mut buffer = Buffer::empty(area);
            ReviewList {
                painter,
                numbered: false,
                active: true,
                items: vec![
                    ReviewItem {
                        path: &path,
                        line: Some(9),
                        mark: Mark::Ticked,
                        text: display::Line::single(display::Role::Plain, "first()"),
                    },
                    ReviewItem {
                        path: &path,
                        line: Some(999),
                        mark: Mark::Unticked,
                        text: display::Line::single(display::Role::Plain, "chosen()"),
                    },
                    ReviewItem {
                        path: &other,
                        line: Some(1),
                        mark: Mark::Ticked,
                        text: display::Line::single(display::Role::Plain, "other()"),
                    },
                ],
            }
            .pane(area, Line::from("Matches"), focused, 1)
            .render(area, &mut buffer);
            let marker = (1..area.height - 1)
                .find(|&y| buffer[(1, y)].symbol() == ">")
                .unwrap();
            assert_eq!(
                buffer[(1, marker)].fg,
                painter.selection_marker(focused).fg.unwrap()
            );
            assert_eq!(
                buffer[(1, marker)].bg,
                painter.selection(focused).bg.unwrap()
            );
            let row: String = (1..area.width - 1)
                .map(|x| buffer[(x, marker)].symbol())
                .collect();
            assert!(row.contains("999 chosen()"), "{row}");
            assert!(row.starts_with("> ▫"));
            assert_eq!(buffer[(1, marker - 1)].bg, ratatui::style::Color::Reset);
        }
    }
}
