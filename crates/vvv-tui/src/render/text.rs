//! A text helper: cutting a string to a column budget.

use std::fmt;
use std::fmt::Write as _;

/// Text cut to `width` columns with an ellipsis; ratatui clips silently otherwise.
pub struct Fit<'a>(pub &'a str, pub usize);

impl Fit<'_> {
    /// Wrap explanatory prose at words, splitting long tokens as complete paths.
    pub fn wrapped_words(&self) -> Vec<String> {
        let mut rows = Vec::new();
        let mut row = String::new();
        for word in self.0.split_whitespace() {
            let candidate = if row.is_empty() {
                word.to_owned()
            } else {
                format!("{row} {word}")
            };
            if ratatui::text::Span::raw(&candidate).width() <= self.1.max(1) {
                row = candidate;
            } else {
                if !row.is_empty() {
                    rows.push(std::mem::take(&mut row));
                }
                let mut wrapped = Fit(word, self.1).wrapped();
                row = wrapped.pop().unwrap_or_default();
                rows.extend(wrapped);
            }
        }
        if !row.is_empty() {
            rows.push(row);
        }
        rows
    }

    /// Preserve every character in metadata that cannot fit on a border.
    pub fn wrapped(&self) -> Vec<String> {
        let mut rows = Vec::new();
        let mut row = String::new();
        let mut used = 0;
        for c in self.0.chars() {
            let width = ratatui::text::Span::raw(c.to_string()).width();
            while used + width > self.1.max(1) && !row.is_empty() {
                if let Some(split) = row.rfind('/').map(|i| i + 1) {
                    let rest = row.split_off(split);
                    rows.push(std::mem::replace(&mut row, rest));
                    used = ratatui::text::Span::raw(row.as_str()).width();
                } else {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
            }
            row.push(c);
            used += width;
        }
        if !row.is_empty() {
            rows.push(row);
        }
        rows
    }
}

impl fmt::Display for Fit<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Fit(text, width) = *self;
        if text.chars().count() <= width {
            return f.write_str(text);
        }
        for c in text.chars().take(width.saturating_sub(1)) {
            f.write_char(c)?;
        }
        f.write_char('…')
    }
}
