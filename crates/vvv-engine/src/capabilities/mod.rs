//! Command ownership index. Keep this table current when adding or moving a command.
//!
//! Related queries share a module when their request, answer, execution, and
//! report describe the same concepts. Shared wire data stays in `protocol`.
//!
//! | Command | Owning module (relative to `src/`) |
//! | --- | --- |
//! | `prepare_rename`, `prepare_rewrite`, `prepare_move`, `prepare_move_symbol`, `inspect_plan`, `review_plan`, `apply_plan`, `discard_plan` | `capabilities/plans.rs` |
//! | `validate_plan` | `capabilities/validation.rs` |
//! | `schema` | `capabilities/schema.rs` |
//! | `search_page` | `capabilities/search.rs` |
//! | `context_page` | `capabilities/context.rs` |
//! | `continue` | `capabilities/pagination.rs` |
//! | `expand` | `capabilities/excerpts.rs` |
//! | `context` | `capabilities/context.rs` |
//! | `discover` | `capabilities/discovery.rs` |
//! | `relationships` | `capabilities/relationships.rs` |
//! | `resolve` | `capabilities/navigation.rs` |
//! | `navigate` | `capabilities/navigation.rs` |
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
//! | `symbol_move_candidates` | `capabilities/moves/selection.rs` |
//! | `move --symbol` | `capabilities/moves/symbol.rs` |
//! | `rewrite` | `rewrite.rs` |
//! | `batch` | `batch.rs` |
//! | `history` | `history.rs` |
//! | `undo` | `history.rs` |
//! | `file`, `workspace_files` | `capabilities/file.rs` |

pub(crate) mod declarations;
pub(crate) mod file;
pub(crate) mod imports;
pub(crate) mod moves;
pub(crate) mod rename;
pub(crate) mod search;
pub(crate) mod surface;
pub(crate) mod usage;

pub(crate) mod navigation;
pub(crate) mod semantic;

pub(crate) mod context;

pub(crate) mod discovery;
pub(crate) mod session;

#[cfg(feature = "schema")]
pub(crate) mod schema;

pub(crate) mod excerpts;
pub(crate) mod pagination;

pub(crate) mod incoming;
pub(crate) mod relationships;

pub(crate) mod plans;

pub(crate) mod validation;
