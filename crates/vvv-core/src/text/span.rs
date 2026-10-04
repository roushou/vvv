use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Half-open byte range `[start, end)` inside a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(try_from = "SpanBounds")]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct SpanBounds {
    start: usize,
    end: usize,
}

/// A byte range that cannot address the supplied source.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpanError {
    #[error("reversed byte range {span:?}")]
    Reversed { span: Span },
    #[error("byte range {span:?} exceeds source length {len}")]
    OutOfBounds { span: Span, len: usize },
    #[error("byte range {span:?} splits a UTF-8 character")]
    CharBoundary { span: Span },
}

impl TryFrom<SpanBounds> for Span {
    type Error = SpanError;

    fn try_from(bounds: SpanBounds) -> Result<Self, Self::Error> {
        let span = Self {
            start: bounds.start,
            end: bounds.end,
        };
        span.validate()?;
        Ok(span)
    }
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        assert!(start <= end, "span start {start} > end {end}");
        Self { start, end }
    }

    pub fn validate(&self) -> Result<(), SpanError> {
        if self.start > self.end {
            return Err(SpanError::Reversed { span: *self });
        }
        Ok(())
    }

    /// Validate ordering, bounds, and character boundaries before slicing.
    pub fn validate_in(&self, source: &str) -> Result<(), SpanError> {
        self.validate()?;
        if self.end > source.len() {
            return Err(SpanError::OutOfBounds {
                span: *self,
                len: source.len(),
            });
        }
        if !source.is_char_boundary(self.start) || !source.is_char_boundary(self.end) {
            return Err(SpanError::CharBoundary { span: *self });
        }
        Ok(())
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
    fn deserialization_rejects_reversed_ranges() {
        assert!(serde_json::from_str::<Span>(r#"{"start":2,"end":1}"#).is_err());
        assert_eq!(
            serde_json::from_str::<Span>(r#"{"start":2,"end":2}"#).unwrap(),
            Span::new(2, 2)
        );
    }

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
