//! Moving files, directories, and declarations, with shared rebasing and reach.

mod extraction;
mod file;
mod reachability;
mod rebase;
mod set;
mod symbol;
mod widen;

pub use extraction::ExtractionError;
pub use file::{Move, MoveIntent};
pub use symbol::{MoveSymbol, MoveSymbolIntent};

pub(crate) use extraction::Extraction;
pub(crate) use reachability::Reachability;
pub(crate) use rebase::{Rebase, Site};
pub(crate) use set::MoveSet;
pub(crate) use widen::Widen;
