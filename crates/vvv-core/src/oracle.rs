//! A second opinion on what a token refers to, from something that knows
//! more than syntax — a compiler, a language server, an index. The graph
//! judges every token by the file's imports and the layout; where that
//! leaves a token unresolved, an oracle may say what it is.

use crate::paths::RelPath;
use std::path::Path;

use crate::text::Span;

/// Where a declaration is: the file and the span of its name, both as the
/// workspace spells them (relative paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Referent {
    pub path: RelPath,
    pub name_span: Span,
}

/// Something that can say what an identifier refers to. Asked one token at
/// a time, only for tokens the graph could not judge, and never trusted
/// blindly: the engine maps the answer onto a declaration it knows before
/// it counts. Implementations are free to answer `None` for anything.
pub trait Oracle: Send + Sync {
    /// The declaration the identifier at `span` in `file` refers to, when
    /// the oracle knows.
    fn refers(&self, file: &Path, span: Span) -> Option<Referent>;
}
