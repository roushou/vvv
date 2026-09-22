//! A small crate with every shape of import vvv follows: child modules,
//! re-exports in chains, a glob re-export, a re-export under another name.

pub mod geometry;
pub mod util;
mod internal;

pub use geometry::{shape::Shape, Point};
pub use internal::Secret as Hidden;
pub use util::*;
