//! Byte-oriented text primitives shared by every layer.

mod line_index;
mod position;
mod source_text;
mod span;

pub use line_index::LineIndex;
pub use position::Position;
pub use source_text::SourceText;
pub use span::Span;
