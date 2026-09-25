//! Colour policy and the palette every human-facing view draws from.

use std::fmt;

use anstyle::{AnsiColor, Color, Style};
use clap::ColorChoice;

use vvv_engine::protocol::display;
use vvv_engine::protocol::vocabulary::Mark;

/// Named styles. One palette per output stream, so stdout can be coloured
/// while stderr is piped, or vice versa.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub path: Style,
    pub lineno: Style,
    pub hit: Style,
    pub symbol: Style,
    /// `●`, and the `kind name` next to it.
    pub declaration: Style,
    /// `→`, and the import row it leads.
    pub import: Style,
    /// `◆ address`.
    pub address: Style,
    /// Row numbers.
    pub ordinal: Style,
    pub added: Style,
    pub removed: Style,
    pub hunk: Style,
    pub title: Style,
    pub error: Style,
    pub warning: Style,
    pub hint: Style,
    pub dim: Style,
    pub strong: Style,
}

impl Palette {
    pub fn plain() -> Self {
        let none = Style::new();
        Self {
            path: none,
            lineno: none,
            hit: none,
            symbol: none,
            declaration: none,
            import: none,
            address: none,
            ordinal: none,
            added: none,
            removed: none,
            hunk: none,
            title: none,
            error: none,
            warning: none,
            hint: none,
            dim: none,
            strong: none,
        }
    }

    pub fn colored() -> Self {
        let fg = |c: AnsiColor| Style::new().fg_color(Some(Color::Ansi(c)));
        Self {
            path: fg(AnsiColor::Magenta).bold(),
            lineno: fg(AnsiColor::Green),
            hit: fg(AnsiColor::Red).bold(),
            symbol: fg(AnsiColor::Cyan),
            declaration: fg(AnsiColor::Cyan).bold(),
            import: Style::new().dimmed(),
            address: Style::new().bold(),
            ordinal: Style::new().dimmed(),
            added: fg(AnsiColor::Green),
            removed: fg(AnsiColor::Red),
            hunk: fg(AnsiColor::Cyan),
            title: Style::new().bold(),
            error: fg(AnsiColor::Red).bold(),
            warning: fg(AnsiColor::Yellow).bold(),
            hint: fg(AnsiColor::Cyan),
            dim: Style::new().dimmed(),
            strong: Style::new().bold(),
        }
    }

    /// Resolve `--color` against the environment for a stream.
    pub fn for_stream(choice: ColorChoice, stream_is_terminal: bool) -> Self {
        if Self::enabled(choice, stream_is_terminal) {
            Self::colored()
        } else {
            Self::plain()
        }
    }

    /// The one colour policy, for the reporters and the picker alike:
    /// `--color`, then `NO_COLOR` and a dumb `TERM`.
    pub fn enabled(choice: ColorChoice, stream_is_terminal: bool) -> bool {
        match choice {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                stream_is_terminal
                    && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
                    && std::env::var("TERM").is_ok_and(|t| t != "dumb")
            }
        }
    }

    /// The colour a mark of the vocabulary takes.
    pub fn mark(&self, mark: Mark) -> Style {
        match mark {
            Mark::Declaration | Mark::OtherDeclaration => self.declaration,
            Mark::Import | Mark::Path | Mark::Glob | Mark::External | Mark::ImportedBy => {
                self.import
            }
            Mark::ReExport | Mark::Oracle => self.path,
            Mark::Safe => self.added,
            Mark::Unverified => self.warning,
            Mark::Other => self.removed,
            Mark::Nothing | Mark::ByName | Mark::Unticked => self.dim,
            Mark::ByHand => self.warning,
            Mark::Structure | Mark::Rewrite => self.hunk,
            Mark::Ticked => self.strong,
            Mark::Undo => self.hint,
        }
    }

    /// The colour a semantic role takes.
    pub fn role(&self, role: display::Role) -> Style {
        match role {
            display::Role::Plain => Style::new(),
            display::Role::Dim => self.dim,
            display::Role::Title => self.title,
            display::Role::Strong => self.strong,
            display::Role::Path => self.path,
            display::Role::LineNumber => self.lineno,
            display::Role::Ordinal => self.ordinal,
            display::Role::Symbol => self.symbol,
            display::Role::Declaration => self.declaration,
            display::Role::Import => self.import,
            display::Role::Address => self.address,
            display::Role::Error => self.error,
            display::Role::Warning => self.warning,
            display::Role::Hint => self.hint,
            display::Role::Hit => self.hit,
            display::Role::Added => self.added,
            display::Role::Removed => self.removed,
            display::Role::Hunk => self.hunk,
            display::Role::Mark(mark) => self.mark(mark),
        }
    }

    /// Wrap `text` so it renders in `style`.
    pub fn paint<T: fmt::Display>(&self, style: Style, text: T) -> Painted<T> {
        Painted { style, text }
    }
}

pub struct Painted<T> {
    style: Style,
    text: T,
}

impl<T: fmt::Display> fmt::Display for Painted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}",
            self.style.render(),
            self.text,
            self.style.render_reset()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_palette_adds_no_escape_codes() {
        let p = Palette::plain();
        assert_eq!(p.paint(p.hit, "x").to_string(), "x");
    }

    #[test]
    fn colored_palette_wraps_in_escapes() {
        let p = Palette::colored();
        let s = p.paint(p.hit, "x").to_string();
        assert!(s.starts_with("\x1b[") && s.ends_with("\x1b[0m") && s.contains('x'));
    }
}
