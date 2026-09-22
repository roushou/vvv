//! The renderer: the drawing primitives that turn a [`Document`] into text,
//! and the [`Renderer`] port an interface implements.
//!
//! [`Document`]: vvv_engine::report::Document

mod palette;
mod style;

use std::io;

use vvv_engine::report::Document;

pub use palette::Palette;
pub use style::Styled;

/// An interface renders a report: it decides the layout, the colour and the
/// sink. It never names a command.
pub trait Renderer {
    fn render(&mut self, report: &Document) -> io::Result<()>;
}
