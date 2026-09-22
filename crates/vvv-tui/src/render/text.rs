//! A text helper: cutting a string to a column budget.

use std::fmt;
use std::fmt::Write as _;

/// Text cut to `width` columns with an ellipsis; ratatui clips silently otherwise.
pub struct Fit<'a>(pub &'a str, pub usize);

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
