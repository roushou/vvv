//! The text a declaration takes with it when it moves.

use std::collections::BTreeSet;

use vvv_core::{Edit, Facts, Span, Symbol, SymbolKind};

/// A declaration and its pieces — in Rust, the `impl` blocks for it — as
/// spans of the file it leaves. Knows what is inside the moved text, what
/// names it uses, how to cut it out and how to paste it elsewhere.
pub(crate) struct Extraction<'a> {
    pub symbol: &'a Symbol,
    /// The symbol's own extent and its companions, in source order.
    pieces: Vec<&'a Symbol>,
    text: &'a str,
}

impl<'a> Extraction<'a> {
    /// The addressable declaration called `name` among `facts`, with its
    /// pieces; `None` when the file declares no such thing.
    pub fn of(
        facts: &'a Facts,
        text: &'a str,
        name: &str,
        is_addressable: impl Fn(SymbolKind) -> bool,
    ) -> Option<Self> {
        let symbol = facts
            .symbols
            .iter()
            .find(|s| s.name == name && is_addressable(s.kind))?;
        let mut pieces: Vec<&Symbol> = facts
            .symbols
            .iter()
            .filter(|s| {
                s.name == name && s.kind != SymbolKind::Method && s.kind != SymbolKind::Field
            })
            .collect();
        pieces.sort_by_key(|s| s.extent.start);
        pieces.dedup_by_key(|s| s.extent.start);
        Some(Self {
            symbol,
            pieces,
            text,
        })
    }

    /// Whether `span` lies inside the moved text.
    pub fn contains(&self, span: Span) -> bool {
        self.pieces
            .iter()
            .any(|p| p.extent.start <= span.start && span.end <= p.extent.end)
    }

    /// Distinct identifier texts inside the moved text: what it may need
    /// imported where it lands.
    pub fn names_used(&self, facts: &Facts) -> BTreeSet<String> {
        facts
            .tokens()
            .filter(|(_, _, span)| self.contains(*span))
            .map(|(name, _, _)| name.to_owned())
            .collect()
    }

    /// The spans to delete from the old file: each piece's extent plus the
    /// newline that ends it, and one blank line after it when there is one,
    /// so the gap closes cleanly.
    pub fn cuts(&self) -> impl Iterator<Item = Span> + '_ {
        self.pieces.iter().map(|piece| {
            let mut end = piece.extent.end;
            if self.text[end..].starts_with('\n') {
                end += 1;
                if self.text[end..].starts_with('\n') {
                    end += 1;
                }
            }
            Span::new(piece.extent.start, end)
        })
    }

    /// The moved text with `edits` (in the old file's coordinates, all inside
    /// the pieces) applied, pieces joined by a blank line.
    pub fn assemble(&self, edits: &[Edit]) -> String {
        let mut out = String::new();
        for (i, piece) in self.pieces.iter().enumerate() {
            if i > 0 {
                out.push_str("\n\n");
            }
            let mut cursor = piece.extent.start;
            let mut inside: Vec<&Edit> = edits
                .iter()
                .filter(|e| e.span.start >= piece.extent.start && e.span.end <= piece.extent.end)
                .collect();
            inside.sort_by_key(|e| e.span.start);
            for edit in inside {
                out.push_str(&self.text[cursor..edit.span.start]);
                out.push_str(&edit.replacement);
                cursor = edit.span.end;
            }
            out.push_str(&self.text[cursor..piece.extent.end]);
        }
        out
    }
}
