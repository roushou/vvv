//! The text a declaration takes with it when it moves.

use std::collections::BTreeSet;

use vvv_core::{Edit, Facts, RelPath, Span, Symbol, SymbolKind};

use crate::SourceFile;

/// Invalid declaration extents or edits in the text selected for a move.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExtractionError {
    #[error("{}: invalid source span {span:?}", path.display())]
    InvalidSpan { path: RelPath, span: Span },
    #[error("{}: declaration pieces overlap: {first:?} and {second:?}", path.display())]
    OverlappingPieces {
        path: RelPath,
        first: Span,
        second: Span,
    },
    #[error("{}: edit {span:?} is outside the moved pieces", path.display())]
    OutsidePieces { path: RelPath, span: Span },
    #[error("{}: moving edits overlap: {first:?} and {second:?}", path.display())]
    OverlappingEdits {
        path: RelPath,
        first: Span,
        second: Span,
    },
}

/// A declaration and its pieces — in Rust, the `impl` blocks for it — as
/// spans of the file it leaves. Knows what is inside the moved text, what
/// names it uses, how to cut it out and how to paste it elsewhere.
pub(crate) struct Extraction<'a> {
    pub symbol: &'a Symbol,
    /// The symbol's own extent and its companions, in source order.
    pieces: Vec<&'a Symbol>,
    source: &'a SourceFile,
}

impl<'a> Extraction<'a> {
    /// The addressable declaration called `name` among `facts`, with its
    /// pieces; `None` when the file declares no such thing.
    pub fn of(
        facts: &'a Facts,
        source: &'a SourceFile,
        name: &str,
        is_addressable: impl Fn(SymbolKind) -> bool,
    ) -> Result<Option<Self>, ExtractionError> {
        let Some(symbol) = facts
            .symbols
            .iter()
            .find(|s| s.name == name && is_addressable(s.kind))
        else {
            return Ok(None);
        };
        let mut pieces: Vec<&Symbol> = facts
            .symbols
            .iter()
            .filter(|s| {
                s.name == name && s.kind != SymbolKind::Method && s.kind != SymbolKind::Field
            })
            .collect();
        pieces.sort_by_key(|s| s.extent);
        pieces.dedup_by_key(|s| s.extent);
        let extraction = Self {
            symbol,
            pieces,
            source,
        };
        for piece in &extraction.pieces {
            extraction.validate_span(piece.extent)?;
        }
        for pair in extraction.pieces.windows(2) {
            if pair[0].extent.overlaps(&pair[1].extent) {
                return Err(ExtractionError::OverlappingPieces {
                    path: source.path().into(),
                    first: pair[0].extent,
                    second: pair[1].extent,
                });
            }
        }
        Ok(Some(extraction))
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
        self.pieces.iter().enumerate().map(|(i, piece)| {
            let mut end = piece.extent.end;
            if self.source.text()[end..].starts_with('\n') {
                end += 1;
                if self.source.text()[end..].starts_with('\n') {
                    end += 1;
                }
            }
            if let Some(next) = self.pieces.get(i + 1) {
                end = end.min(next.extent.start);
            }
            Span::new(piece.extent.start, end)
        })
    }

    /// The moved text with `edits` (in the old file's coordinates, all inside
    /// the pieces) applied, pieces joined by a blank line.
    pub fn assemble(&self, edits: &[Edit]) -> Result<String, ExtractionError> {
        let mut assigned = vec![Vec::<&Edit>::new(); self.pieces.len()];
        for edit in edits {
            self.validate_span(edit.span)?;
            // At a shared boundary an insertion belongs to the following piece.
            let owner = self
                .pieces
                .iter()
                .position(|p| p.extent.start == edit.span.start && p.extent.contains(&edit.span))
                .or_else(|| {
                    self.pieces
                        .iter()
                        .position(|p| p.extent.contains(&edit.span))
                });
            let Some(owner) = owner else {
                return Err(ExtractionError::OutsidePieces {
                    path: self.source.path().into(),
                    span: edit.span,
                });
            };
            for previous in &assigned[owner] {
                if previous.span.overlaps(&edit.span) {
                    return Err(ExtractionError::OverlappingEdits {
                        path: self.source.path().into(),
                        first: previous.span,
                        second: edit.span,
                    });
                }
            }
            assigned[owner].push(edit);
        }
        let mut out = String::new();
        for (i, (piece, inside)) in self.pieces.iter().zip(&mut assigned).enumerate() {
            if i > 0 {
                out.push_str("\n\n");
            }
            // Stable order preserves coincident insertions, before a replacement
            // beginning at the same byte. End-boundary insertions follow it.
            inside.sort_by_key(|e| (e.span.start, !e.span.is_empty()));
            let mut cursor = piece.extent.start;
            for edit in inside {
                out.push_str(&self.source.text()[cursor..edit.span.start]);
                out.push_str(&edit.replacement);
                cursor = edit.span.end;
            }
            out.push_str(&self.source.text()[cursor..piece.extent.end]);
        }
        Ok(out)
    }

    fn validate_span(&self, span: Span) -> Result<(), ExtractionError> {
        let text = self.source.text();
        if span.start > span.end
            || span.end > text.len()
            || !text.is_char_boundary(span.start)
            || !text.is_char_boundary(span.end)
        {
            return Err(ExtractionError::InvalidSpan {
                path: self.source.path().into(),
                span,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        source: SourceFile,
        facts: Facts,
    }

    impl Fixture {
        fn new(text: &str, extents: &[Span]) -> Self {
            Self {
                source: SourceFile::new("source.fake", text),
                facts: Facts::new(
                    extents
                        .iter()
                        .map(|&span| Symbol::plain(SymbolKind::Function, "f", span, span))
                        .collect(),
                    vec![],
                    vec![],
                ),
            }
        }

        fn extraction(&self) -> Result<Extraction<'_>, ExtractionError> {
            Ok(Extraction::of(&self.facts, &self.source, "f", |_| true)?.unwrap())
        }
    }

    #[test]
    fn moving_edits_must_not_overlap() {
        let fixture = Fixture::new("abcdef", &[Span::new(0, 6)]);
        let result = fixture.extraction().unwrap().assemble(&[
            Edit::replace(Span::new(1, 4), "x"),
            Edit::replace(Span::new(3, 5), "y"),
        ]);
        assert!(
            matches!(result, Err(ExtractionError::OverlappingEdits { first, second, .. })
            if first == Span::new(1, 4) && second == Span::new(3, 5))
        );
    }

    #[test]
    fn every_moving_edit_belongs_to_a_piece() {
        let fixture = Fixture::new("abcdef", &[Span::new(0, 2), Span::new(4, 6)]);
        assert!(matches!(
            fixture
                .extraction()
                .unwrap()
                .assemble(&[Edit::replace(Span::new(1, 5), "x")]),
            Err(ExtractionError::OutsidePieces { .. })
        ));
    }

    #[test]
    fn extents_and_edits_are_valid_source_ranges() {
        for span in [Span { start: 3, end: 1 }, Span::new(0, 8), Span::new(1, 2)] {
            let fixture = Fixture::new("éabc", &[span]);
            assert!(
                matches!(fixture.extraction(), Err(ExtractionError::InvalidSpan { span: invalid, .. }) if invalid == span)
            );
            let fixture = Fixture::new("éabc", &[Span::new(0, 5)]);
            assert!(
                matches!(fixture.extraction().unwrap().assemble(&[Edit::replace(span, "x")]),
                Err(ExtractionError::InvalidSpan { span: invalid, .. }) if invalid == span)
            );
        }
    }

    #[test]
    fn distinct_piece_extents_must_not_overlap() {
        let fixture = Fixture::new("abcdef", &[Span::new(0, 3), Span::new(0, 5)]);
        assert!(matches!(
            fixture.extraction(),
            Err(ExtractionError::OverlappingPieces { .. })
        ));
        let fixture = Fixture::new("abcdef", &[Span::new(0, 3), Span::new(2, 5)]);
        assert!(matches!(
            fixture.extraction(),
            Err(ExtractionError::OverlappingPieces { .. })
        ));
    }

    #[test]
    fn identical_pieces_are_extracted_once() {
        let fixture = Fixture::new(
            "abcdef",
            &[Span::new(0, 3), Span::new(0, 3), Span::new(4, 6)],
        );
        assert_eq!(
            fixture.extraction().unwrap().assemble(&[]).unwrap(),
            "abc\n\nef"
        );
    }

    #[test]
    fn boundary_and_coincident_insertions_keep_their_order() {
        let fixture = Fixture::new("abcdef", &[Span::new(0, 6)]);
        let edits = [
            Edit::replace(Span::new(1, 3), "X"),
            Edit::insert(1, "A"),
            Edit::insert(1, "B"),
            Edit::insert(3, "C"),
        ];
        assert_eq!(
            fixture.extraction().unwrap().assemble(&edits).unwrap(),
            "aABXCdef"
        );
    }

    #[test]
    fn a_shared_boundary_insertion_is_applied_once() {
        let fixture = Fixture::new("abcd", &[Span::new(0, 2), Span::new(2, 4)]);
        assert_eq!(
            fixture
                .extraction()
                .unwrap()
                .assemble(&[Edit::insert(2, "X")])
                .unwrap(),
            "ab\n\nXcd"
        );
    }

    #[test]
    fn edits_in_multiple_pieces_keep_source_coordinates() {
        let fixture = Fixture::new("abc---def", &[Span::new(0, 3), Span::new(6, 9)]);
        assert_eq!(
            fixture
                .extraction()
                .unwrap()
                .assemble(&[
                    Edit::replace(Span::new(1, 2), "B"),
                    Edit::replace(Span::new(7, 8), "E")
                ])
                .unwrap(),
            "aBc\n\ndEf"
        );
    }
}
