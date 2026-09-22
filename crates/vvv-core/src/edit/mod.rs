//! Text edits and the [`ChangeSet`] every mutating refactor produces.
//!
//! Planners never write files; they emit a `ChangeSet`. Previewing and
//! applying are separate steps performed on that value.

mod change_set;

use serde::{Deserialize, Serialize};

pub use change_set::{ChangeSet, EditConflict};

use crate::text::Span;

/// Replace the bytes at `span` with `replacement`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub span: Span,
    pub replacement: String,
}

impl Edit {
    pub fn replace(span: Span, replacement: impl Into<String>) -> Self {
        Self {
            span,
            replacement: replacement.into(),
        }
    }

    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Self::replace(Span::new(at, at), text)
    }

    pub fn delete(span: Span) -> Self {
        Self::replace(span, "")
    }
}
