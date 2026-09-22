use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Half-open byte range `[start, end)` inside a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        debug_assert!(start <= end, "span start {start} > end {end}");
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Two spans overlap when they share at least one byte. Touching spans do not.
    pub fn overlaps(&self, other: &Span) -> bool {
        self.start < other.end && other.start < self.end
    }

    pub fn contains(&self, other: &Span) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// Whether the byte at `offset` lies inside the span.
    pub fn contains_offset(&self, offset: usize) -> bool {
        (self.start..self.end).contains(&offset)
    }

    /// Smallest span covering both.
    pub fn union(&self, other: &Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

impl From<Range<usize>> for Span {
    fn from(range: Range<usize>) -> Self {
        Span::new(range.start, range.end)
    }
}

impl From<Span> for Range<usize> {
    fn from(span: Span) -> Self {
        span.start..span.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_strict() {
        let a = Span::new(0, 5);
        assert!(a.overlaps(&Span::new(4, 8)));
        assert!(!a.overlaps(&Span::new(5, 8)));
        assert!(!Span::new(5, 8).overlaps(&a));
    }

    #[test]
    fn empty_span_conflicts_inside_a_range_but_not_at_its_edges() {
        let a = Span::new(0, 5);
        assert!(a.overlaps(&Span::new(2, 2)));
        assert!(!a.overlaps(&Span::new(0, 0)));
        assert!(!a.overlaps(&Span::new(5, 5)));
        assert!(!Span::new(3, 3).overlaps(&Span::new(3, 3)));
    }
}
