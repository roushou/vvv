//! Making a declaration visible to the modules that will name it.

use std::path::PathBuf;

use vvv_core::{Address, Edit, Position, ReachKind, SourceText, Surgery, Symbol};

use crate::{Notice, NoticeKind};

/// A declaration whose reach no longer admits one of its consumers, and the
/// narrowest reach that would: the surgery spells the wider modifier, or —
/// across a package boundary, or in a language that cannot write it — a
/// human is told instead.
pub(crate) struct Widen<'a> {
    pub symbol: &'a Symbol,
    /// The file the modifier is written in.
    pub source: &'a SourceText,
    pub needs: ReachKind,
    /// The module the declaration must be seen from: what the notice names.
    pub consumer: Address,
    /// Where the notice points when the widening cannot be written: the
    /// modifier's own site, unless the item is about to move elsewhere.
    pub notice_at: (PathBuf, Position),
}

impl Widen<'_> {
    /// The edit that widens, or the notice that says it cannot be done.
    /// `Everyone` is never inferred: making a declaration public across
    /// packages is the author's call.
    pub fn plan(&self, surgery: &dyn Surgery) -> Result<Edit, Notice> {
        match surgery.widen(self.symbol, self.source, self.needs) {
            Some(edit) if self.needs != ReachKind::Everyone => Ok(edit),
            _ => Err(Notice {
                path: self.notice_at.0.clone().into(),
                start: self.notice_at.1,
                kind: NoticeKind::Unreachable {
                    item: self.symbol.name.clone(),
                    from: self.consumer.clone(),
                    needs: self.needs,
                },
            }),
        }
    }
}
