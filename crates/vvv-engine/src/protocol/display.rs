//! A styled line as data: the words and their roles, no colours. An
//! interface maps a [`Role`] to its own style and renders the line, so the
//! composition of a row — what is written, in what order, which part is the
//! hit — lives once, in the protocol.

use super::Match;
use super::vocabulary::Mark;

/// The semantic role of a run of text. A backend names the colour; the data
/// never carries one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// No style of its own.
    Plain,
    /// A word around the answer: a label, a count.
    Dim,
    /// A heading or a section label.
    Title,
    /// An emphasised word: a count, a history id, a module header.
    Strong,
    /// A file path.
    Path,
    /// A line number.
    LineNumber,
    /// A row's number.
    Ordinal,
    /// A declaration's modifier.
    Symbol,
    /// `●` and a declaration's `kind name`.
    Declaration,
    /// An import, or the text around one.
    Import,
    /// `◆ address`.
    Address,
    /// `✗` and an error.
    Error,
    /// `!` and a warning.
    Warning,
    /// `hint:` and what to try instead.
    Hint,
    /// The hit within a line.
    Hit,
    /// A line a change adds.
    Added,
    /// A line a change removes.
    Removed,
    /// A diff hunk header.
    Hunk,
    /// A mark's glyph (`●`, `✓`, `→`): the renderer colours it by the mark.
    Mark(Mark),
}

/// A run of text with one role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub role: Role,
}

impl Piece {
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            role,
        }
    }
}

/// One line of output: its pieces, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pieces: Vec<Piece>,
}

impl Line {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn of(role: Role, text: impl Into<String>) -> Self {
        Self {
            pieces: vec![Piece::new(role, text)],
        }
    }

    /// A match's source line split around the hit, leading indentation
    /// dropped: the text before, the hit, the text after. The text around the
    /// hit takes `rest`.
    pub fn hit(m: &Match, rest: Role) -> Self {
        let chars: Vec<char> = m.line.chars().collect();
        let start = (m.start.column as usize).min(chars.len());
        let stop = if m.end.line == m.start.line {
            (m.end.column as usize).clamp(start, chars.len())
        } else {
            chars.len()
        };
        let indent = chars[..start]
            .iter()
            .take_while(|c| c.is_whitespace())
            .count();
        let before: String = chars[indent..start].iter().collect();
        let token: String = chars[start..stop].iter().collect();
        let after: String = chars[stop..].iter().collect();
        Self::new()
            .and(rest, before)
            .and(Role::Hit, token)
            .and(rest, after)
    }

    /// A mark's glyph as a line.
    pub fn mark(mark: Mark) -> Self {
        Self::of(Role::Mark(mark), mark.glyph().to_string())
    }

    /// `✓ 3  ? 1  ✗ 0`: each mark's glyph and count, pairs two spaces apart.
    pub fn counts(counts: &[(Mark, usize)]) -> Self {
        let mut line = Self::new();
        for (i, (mark, count)) in counts.iter().enumerate() {
            if i > 0 {
                line = line.and(Role::Plain, "  ");
            }
            line = line
                .and(Role::Mark(*mark), format!("{} ", mark.glyph()))
                .and(Role::Plain, count.to_string());
        }
        line
    }

    /// Add a piece; the line, for chaining.
    pub fn and(mut self, role: Role, text: impl Into<String>) -> Self {
        self.pieces.push(Piece::new(role, text));
        self
    }

    /// Append another line's pieces; the line, for chaining.
    pub fn and_line(mut self, other: Line) -> Self {
        self.pieces.extend(other.pieces);
        self
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.iter().all(|p| p.text.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use vvv_core::RelPath;

    use vvv_core::{LanguageId, Position, Role as MatchRole, Span};

    use super::*;
    use crate::protocol::MatchId;

    fn at(line: &str, start: u32, end: u32) -> Match {
        Match {
            id: MatchId::from("test".to_owned()),
            path: RelPath::from("a.rs"),
            language: LanguageId::from("rust"),
            span: Span::new(0, 0),
            start: Position::new(0, start),
            end: Position::new(0, end),
            kind: "identifier".to_owned(),
            text: String::new(),
            line: line.to_owned(),
            captures: BTreeMap::new(),
            symbol: None,
            role: MatchRole::Use,
            address: None,
        }
    }

    #[test]
    fn hit_uses_char_columns_and_trims_leading_indent() {
        let line = Line::hit(&at("    let é = foo(1);", 12, 15), Role::Plain);
        let text: String = line.pieces().iter().map(|p| p.text.as_str()).collect();
        assert_eq!(text, "let é = foo(1);");
        assert_eq!(line.pieces()[1].role, Role::Hit);
        assert_eq!(line.pieces()[1].text, "foo");
    }

    #[test]
    fn counts_puts_a_marks_glyph_beside_its_count() {
        let line = Line::counts(&[(Mark::Safe, 3), (Mark::Unverified, 1), (Mark::Other, 0)]);
        let text: String = line.pieces().iter().map(|p| p.text.as_str()).collect();
        assert_eq!(text, "✓ 3  ? 1  ✗ 0");
        assert_eq!(line.pieces()[0].role, Role::Mark(Mark::Safe));
        assert_eq!(line.pieces()[1].role, Role::Plain);
    }
}
