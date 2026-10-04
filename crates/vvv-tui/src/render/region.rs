//! The rectangle a screen lays its panels out in.

use ratatui::layout::{Constraint, Layout, Rect};

/// A drawing area, with the splits a screen arranges panels with. Layout
/// works in `Region`s; a `Rect` is what a widget renders into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region(Rect);

impl Region {
    pub const fn new(rect: Rect) -> Self {
        Self(rect)
    }

    pub const fn rect(self) -> Rect {
        self.0
    }

    /// One panel using the full area, as overlay views do.
    pub fn full(self) -> Vec<Self> {
        vec![self]
    }

    /// The left and right of the area, `percent` of the width to the left.
    pub fn columns(self, percent: u16) -> (Region, Region) {
        let [left, right] = Layout::horizontal([
            Constraint::Percentage(percent),
            Constraint::Percentage(100 - percent),
        ])
        .areas(self.0);
        (Region(left), Region(right))
    }

    /// The first `height` rows and the rest.
    pub fn split(self, height: u16) -> (Region, Region) {
        let [top, rest] =
            Layout::vertical([Constraint::Length(height), Constraint::Min(1)]).areas(self.0);
        (Region(top), Region(rest))
    }

    /// Review lists keep compact sibling panes and give their active list
    /// the remaining space. Short terminals retain a navigable active pane.
    pub fn review_rows(self, rows: &[usize], active: usize) -> Vec<Region> {
        let compact = self.0.height < (rows.len() * 4) as u16;
        let constraints: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(i, &count)| {
                if i == active {
                    Constraint::Fill(1)
                } else {
                    Constraint::Length(if compact || count == 0 {
                        1
                    } else {
                        (count.saturating_mul(2) + 2).min(4) as u16
                    })
                }
            })
            .collect();
        Layout::vertical(constraints)
            .split(self.0)
            .iter()
            .copied()
            .map(Region)
            .collect()
    }
}
