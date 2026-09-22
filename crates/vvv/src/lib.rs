//! The binary's composition root, and the one piece its integration tests
//! reuse.
//!
//! The binary `vvv` is `src/main.rs`; this library exists so `tests/` can
//! build the same engine the binary ships without a second copy of the
//! plugin list.

mod languages;

pub use languages::Builtins;
