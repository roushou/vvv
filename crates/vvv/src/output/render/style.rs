//! Rendering a [`display::Line`] with a palette.

use std::fmt;

use vvv_engine::protocol::display;

use super::palette::Palette;

/// A [`display::Line`] rendered with a palette: each piece in its role's
/// colour.
pub struct Styled<'a>(pub &'a Palette, pub &'a display::Line);

impl fmt::Display for Styled<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for piece in self.1.pieces() {
            write!(f, "{}", self.0.paint(self.0.role(piece.role), &piece.text))?;
        }
        Ok(())
    }
}
