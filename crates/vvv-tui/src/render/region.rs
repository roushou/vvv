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

    /// Stack panels vertically: collapsed ones one line, the rest sharing the
    /// remaining height by `weight`.
    pub fn rows(self, sizes: &[(usize, u16)]) -> Vec<Region> {
        let constraints: Vec<Constraint> = sizes
            .iter()
            .map(|(rows, weight)| {
                if *rows == 0 {
                    Constraint::Length(1)
                } else {
                    Constraint::Fill(*weight)
                }
            })
            .collect();
        Layout::vertical(constraints)
            .split(self.0)
            .to_vec()
            .into_iter()
            .map(Region)
            .collect()
    }
}
