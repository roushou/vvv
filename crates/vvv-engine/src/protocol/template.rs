use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Match;

use vvv_core::{CaptureValue, SourceText};

/// Replacement text with meta-variable holes: `bar($$$ARGS)`, `$NAME_v2`.
///
/// Variables are `$NAME` (single capture) or `$$$NAME` (sequence capture);
/// a name is `[A-Z_][A-Z0-9_]*`. Anything else is literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Template {
    source: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TemplateError {
    #[error("template refers to `${0}` but the match has no such capture")]
    UnknownVariable(String),
}

#[derive(Debug, PartialEq, Eq)]
enum Piece<'t> {
    Literal(&'t str),
    Variable(&'t str),
}

impl Template {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Fill the holes with the captures of `m`, reading sequence captures
    /// from `source` so the original punctuation between items is kept.
    pub fn expand(&self, m: &Match, source: &SourceText) -> Result<String, TemplateError> {
        let mut out = String::with_capacity(self.source.len());
        for piece in self.pieces() {
            match piece {
                Piece::Literal(text) => out.push_str(text),
                Piece::Variable(name) => match m.capture(name) {
                    Some(CaptureValue::Single(c)) => out.push_str(&c.text),
                    Some(CaptureValue::Multiple(items)) => {
                        if let (Some(first), Some(last)) = (items.first(), items.last()) {
                            out.push_str(source.slice(first.span.union(&last.span)));
                        }
                    }
                    None => return Err(TemplateError::UnknownVariable(name.to_owned())),
                },
            }
        }
        Ok(out)
    }

    fn pieces(&self) -> Vec<Piece<'_>> {
        let src = self.source.as_str();
        let mut pieces = Vec::new();
        let mut literal_start = 0;
        let mut i = 0;
        while i < src.len() {
            if src.as_bytes()[i] != b'$' {
                i += 1;
                continue;
            }
            let dollars = src[i..].bytes().take_while(|&b| b == b'$').count();
            let name_start = i + dollars;
            let name_len = Self::variable_name_len(&src[name_start..]);
            if (dollars == 1 || dollars == 3) && name_len > 0 {
                if literal_start < i {
                    pieces.push(Piece::Literal(&src[literal_start..i]));
                }
                pieces.push(Piece::Variable(&src[name_start..name_start + name_len]));
                i = name_start + name_len;
                literal_start = i;
            } else {
                i = name_start.max(i + 1);
            }
        }
        if literal_start < src.len() {
            pieces.push(Piece::Literal(&src[literal_start..]));
        }
        pieces
    }

    fn variable_name_len(s: &str) -> usize {
        let mut bytes = s.bytes();
        match bytes.next() {
            Some(b) if b.is_ascii_uppercase() || b == b'_' => {}
            _ => return 0,
        }
        1 + bytes
            .take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
            .count()
    }
}

impl fmt::Display for Template {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl From<&str> for Template {
    fn from(s: &str) -> Self {
        Template::new(s)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::MatchId;
    use vvv_core::{Capture, LanguageId, Position, Span};

    fn fixture() -> (Match, SourceText) {
        let source = SourceText::new("foo(a, b)");
        let captures = BTreeMap::from([
            (
                "F".to_owned(),
                CaptureValue::Single(Capture {
                    span: Span::new(0, 3),
                    text: "foo".into(),
                }),
            ),
            (
                "ARGS".to_owned(),
                CaptureValue::Multiple(vec![
                    Capture {
                        span: Span::new(4, 5),
                        text: "a".into(),
                    },
                    Capture {
                        span: Span::new(5, 6),
                        text: ",".into(),
                    },
                    Capture {
                        span: Span::new(7, 8),
                        text: "b".into(),
                    },
                ]),
            ),
            ("NONE".to_owned(), CaptureValue::Multiple(vec![])),
        ]);
        let m = Match {
            id: MatchId::derive(&vvv_core::RelPath::from("x"), Span::new(0, 9), "foo(a, b)"),
            path: "x".into(),
            language: LanguageId::new("t"),
            span: Span::new(0, 9),
            start: Position::new(0, 0),
            end: Position::new(0, 9),
            kind: "call".into(),
            text: "foo(a, b)".into(),
            line: "foo(a, b)".into(),
            captures,
            symbol: None,
            role: vvv_core::Role::Use,
            address: None,
        };
        (m, source)
    }

    #[test]
    fn tokenizes_variables_and_literals() {
        let t = Template::new("$F_v2($$$ARGS) $ $$X $lower");
        assert_eq!(
            t.pieces(),
            vec![
                Piece::Variable("F_"),
                Piece::Literal("v2("),
                Piece::Variable("ARGS"),
                Piece::Literal(") $ $$X $lower"),
            ]
        );
    }

    #[test]
    fn expands_single_and_sequence_captures() {
        let (m, src) = fixture();
        let out = Template::new("bar($$$ARGS, $F)[$$$NONE]")
            .expand(&m, &src)
            .unwrap();
        assert_eq!(out, "bar(a, b, foo)[]");
    }

    #[test]
    fn unknown_variable_is_an_error() {
        let (m, src) = fixture();
        assert_eq!(
            Template::new("$NOPE").expand(&m, &src),
            Err(TemplateError::UnknownVariable("NOPE".into()))
        );
    }
}
