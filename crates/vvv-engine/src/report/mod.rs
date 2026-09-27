//! Shared report vocabulary. Capabilities compose answers into [`Document`]s;
//! a [`View`] lays those facts out as rows. Composition stays on `Document`,
//! implemented beside each capability's execution. No report is serialized.

mod block;
mod document;
pub(crate) mod lines;
mod row;
mod view;

pub(crate) use block::MoveCounts;
pub use block::{Block, Note, ReferencePlan};
pub use document::Document;
pub use row::{Row, Source};
pub use view::{Detailed, Options, Presentation, View};
