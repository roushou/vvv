//! Caret editing and a visible window over an input's retained text.
use crate::render::Painter;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCommand {
    Left,
    Right,
    Home,
    End,
    WordLeft,
    WordRight,
    Backspace,
    Delete,
    WordBackspace,
    WordDelete,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_edits_whole_graphemes_and_words_in_place() {
        let mut text = "one e\u{301}👩‍💻界 last".to_owned();
        let mut caret = Caret::default();
        let apply = |text: &mut String, caret: &mut Caret, command| {
            TextInput::new(text, caret).apply(Edit::Command(command))
        };
        assert!(!apply(&mut text, &mut caret, EditCommand::WordLeft));
        assert_eq!(&text[caret.offset(&text)..], "last");
        assert!(TextInput::new(&mut text, &mut caret).apply(Edit::Insert("new ")));
        assert_eq!(text, "one e\u{301}👩‍💻界 new last");
        assert!(apply(&mut text, &mut caret, EditCommand::WordBackspace));
        assert_eq!(text, "one e\u{301}👩‍💻界 last");
        apply(&mut text, &mut caret, EditCommand::Home);
        for _ in 0..4 {
            apply(&mut text, &mut caret, EditCommand::Right);
        }
        assert!(apply(&mut text, &mut caret, EditCommand::Delete));
        assert_eq!(text, "one 👩‍💻界 last");
        assert!(apply(&mut text, &mut caret, EditCommand::Delete));
        assert_eq!(text, "one 界 last");
        apply(&mut text, &mut caret, EditCommand::Right);
        assert!(apply(&mut text, &mut caret, EditCommand::Backspace));
        assert_eq!(text, "one  last");
        apply(&mut text, &mut caret, EditCommand::End);
        assert!(!apply(&mut text, &mut caret, EditCommand::Delete));
        assert!(TextInput::new(&mut text, &mut caret).apply(Edit::Clear));
        assert_eq!(caret.offset(&text), 0);
        assert!(!apply(&mut text, &mut caret, EditCommand::Backspace));
    }

    #[test]
    fn input_windows_fit_every_width_and_keep_the_caret_visible() {
        let text = "crates/界/e\u{301}/👩‍💻/really_long_filename.rs\n$BODY";
        let mut value = text.to_owned();
        let mut caret = Caret::default();
        TextInput::new(&mut value, &mut caret).apply(Edit::Command(EditCommand::Home));
        for _ in 0..=text.graphemes(true).count() {
            for width in 1..50 {
                let line = caret.line(text, Painter::plain(), true, width);
                assert!(line.width() <= width, "{width}: {line:?}");
                assert!(
                    line.to_string().contains('▏'),
                    "caret missing at {}: {line:?}",
                    caret.offset(text)
                );
                assert!(!line.to_string().contains('\n'));
                assert!(caret.line(text, Painter::plain(), false, width).width() <= width);
            }
            TextInput::new(&mut value, &mut caret).apply(Edit::Command(EditCommand::Right));
        }
        assert_eq!(value, text);
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Edit<'a> {
    Insert(&'a str),
    Command(EditCommand),
    Clear,
}
impl Edit<'_> {
    pub fn motion(self) -> bool {
        matches!(
            self,
            Self::Command(
                EditCommand::Left
                    | EditCommand::Right
                    | EditCommand::Home
                    | EditCommand::End
                    | EditCommand::WordLeft
                    | EditCommand::WordRight
            )
        )
    }
}
/// `None` tracks the end, including when another control replaces the buffer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Caret {
    position: Option<usize>,
}
impl Caret {
    pub fn offset(&self, text: &str) -> usize {
        let requested = self.position.unwrap_or(text.len()).min(text.len());
        text.grapheme_indices(true)
            .map(|(i, _)| i)
            .chain([text.len()])
            .take_while(|&i| i <= requested)
            .last()
            .unwrap_or(0)
    }
    pub fn reset(&mut self) {
        self.position = None;
    }
    /// A single-line representation; line breaks remain intact in the buffer.
    pub fn line(&self, text: &str, painter: Painter, focused: bool, width: usize) -> Line<'static> {
        if width == 0 {
            return Line::default();
        }
        let offset = self.offset(text);
        let parts: Vec<_> = text
            .grapheme_indices(true)
            .map(|(i, s)| {
                let visible = s.replace(['\r', '\n'], "↵").replace('\t', "→");
                let columns = Span::raw(visible.clone()).width();
                (i, visible, columns)
            })
            .collect();
        let caret = parts.partition_point(|(i, _, _)| *i < offset);
        let marker = usize::from(focused);
        if width == marker {
            return Line::from(painter.caret(focused));
        }
        let mut start = 0;
        let mut before: usize = parts[..caret].iter().map(|p| p.2).sum();
        let reserve = usize::from(caret < parts.len());
        while start < caret && before + usize::from(start > 0) + marker + reserve > width {
            before -= parts[start].2;
            start += 1;
        }
        let left = usize::from(start > 0 && width > marker);
        let mut used = before + left + marker;
        let mut end = caret;
        while end < parts.len() {
            let tail = usize::from(end + 1 < parts.len());
            if used + parts[end].2 + tail > width {
                break;
            }
            used += parts[end].2;
            end += 1;
        }
        let mut spans = Vec::new();
        if left > 0 {
            spans.push(Span::styled("…", painter.dim));
        }
        for (index, (_, visible, _)) in parts.iter().enumerate().take(end).skip(start) {
            if index == caret && focused {
                spans.push(painter.caret(true));
            }
            spans.push(Span::raw(visible.clone()));
        }
        if caret == end && focused {
            spans.push(painter.caret(true));
        }
        if end < parts.len() && used < width {
            spans.push(Span::styled("…", painter.dim));
        }
        Line::from(spans)
    }
}

pub(crate) struct TextInput<'a> {
    text: &'a mut String,
    caret: &'a mut Caret,
}
impl<'a> TextInput<'a> {
    pub fn new(text: &'a mut String, caret: &'a mut Caret) -> Self {
        Self { text, caret }
    }
    pub fn edit(self, character: Option<char>) {
        if let Some(c) = character {
            self.apply(Edit::Insert(&c.to_string()));
        } else {
            self.apply(Edit::Command(EditCommand::Backspace));
        }
    }
    pub fn apply(self, edit: Edit<'_>) -> bool {
        let at = self.caret.offset(self.text);
        let previous = self.text[..at]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i);
        let next = self.text[at..]
            .graphemes(true)
            .next()
            .map_or(at, |s| at + s.len());
        let word_left = || {
            let mut position = at;
            let mut started = false;
            for (i, g) in self.text[..at].grapheme_indices(true).rev() {
                let word = g.chars().any(|c| c.is_alphanumeric() || c == '_');
                if started && !word {
                    break;
                }
                started |= word;
                position = i;
            }
            position
        };
        let word_right = || {
            let mut position = at;
            let mut started = false;
            for (i, g) in self.text[at..].grapheme_indices(true) {
                let word = g.chars().any(|c| c.is_alphanumeric() || c == '_');
                if started && !word {
                    break;
                }
                started |= word;
                position = at + i + g.len();
            }
            position
        };
        let range = match edit {
            Edit::Insert(text) => {
                if text.is_empty() {
                    return false;
                }
                self.text.insert_str(at, text);
                self.caret.position = Some(at + text.len());
                return true;
            }
            Edit::Clear => 0..self.text.len(),
            Edit::Command(command) => match command {
                EditCommand::Backspace => previous..at,
                EditCommand::Delete => at..next,
                EditCommand::WordBackspace => word_left()..at,
                EditCommand::WordDelete => at..word_right(),
                command => {
                    self.caret.position = match command {
                        EditCommand::Left => Some(previous),
                        EditCommand::Right => Some(next),
                        EditCommand::Home => Some(0),
                        EditCommand::End => None,
                        EditCommand::WordLeft => Some(word_left()),
                        EditCommand::WordRight => Some(word_right()),
                        _ => unreachable!(),
                    };
                    return false;
                }
            },
        };
        if range.is_empty() {
            return false;
        }
        self.caret.position = Some(range.start);
        self.text.replace_range(range, "");
        true
    }
}
