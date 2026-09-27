//! Command ownership index. Keep this table current when adding or moving a command.
//!
//! Related queries share a module when their request, answer, execution, and
//! report describe the same concepts. Shared wire data stays in `protocol`.
//!
//! | Command | Owning module (relative to `src/`) |
//! | --- | --- |
//! | `search` | `capabilities/search.rs` |
//! | `outline` | `capabilities/declarations.rs` |
//! | `where` | `capabilities/declarations.rs` |
//! | `references` | `capabilities/rename.rs` |
//! | `deps` | `capabilities/imports.rs` |
//! | `explain` | `capabilities/imports.rs` |
//! | `imports` | `capabilities/imports.rs` |
//! | `surface` | `capabilities/surface.rs` |
//! | `impact` | `capabilities/usage.rs` |
//! | `dead` | `capabilities/usage.rs` |
//! | `rename` | `capabilities/rename.rs` |
//! | `move` | `capabilities/moves/file.rs` |
//! | `move --symbol` | `capabilities/moves/symbol.rs` |
//! | `rewrite` | `rewrite.rs` |
//! | `batch` | `batch.rs` |
//! | `history` | `history.rs` |
//! | `undo` | `history.rs` |
//! | `file` (picker request) | `capabilities/file.rs` |

pub(crate) mod declarations;
pub(crate) mod file;
pub(crate) mod imports;
pub(crate) mod moves;
pub(crate) mod rename;
pub(crate) mod search;
pub(crate) mod surface;
pub(crate) mod usage;
