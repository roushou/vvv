//! Colours for the TUI. Mirrors the CLI palette's meaning; ratatui styles
//! instead of ANSI strings. Syntax colours are conservative: enough to read
//! code, never louder than the match itself.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use vvv_engine::HighlightKind;
use vvv_engine::protocol::display;
use vvv_engine::protocol::vocabulary::Mark;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub path: Style,
    pub hit: Style,
    pub symbol: Style,
    /// `●` and a declaration's `kind name`.
    pub declaration: Style,
    /// `→` and an import row.
    pub import: Style,
    /// `◆ address`.
    pub address: Style,
    /// `↗`, a re-export.
    pub reexport: Style,
    /// `▪`, a ticked row.
    pub tick: Style,
    pub cursor: Style,
    pub added: Style,
    pub removed: Style,
    pub hunk: Style,
    pub title: Style,
    pub dim: Style,
    pub error: Style,
    pub warning: Style,
    pub key: Style,
    pub focused: Style,
    pub unfocused: Style,
    pub keyword: Style,
    pub string: Style,
    pub comment: Style,
    pub number: Style,
    pub type_name: Style,
    pub function: Style,
    pub macro_name: Style,
    pub attribute: Style,
}

impl Theme {
    pub fn plain() -> Self {
        let none = Style::new();
        Self {
            path: none.add_modifier(Modifier::BOLD),
            hit: none.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            symbol: none,
            declaration: none.add_modifier(Modifier::BOLD),
            import: none.add_modifier(Modifier::DIM),
            address: none.add_modifier(Modifier::BOLD),
            reexport: none,
            tick: none.add_modifier(Modifier::BOLD),
            cursor: none.add_modifier(Modifier::REVERSED),
            added: none,
            removed: none,
            hunk: none,
            title: none.add_modifier(Modifier::BOLD),
            dim: none.add_modifier(Modifier::DIM),
            error: none.add_modifier(Modifier::BOLD),
            warning: none.add_modifier(Modifier::BOLD),
            key: none.add_modifier(Modifier::BOLD),
            focused: none.add_modifier(Modifier::BOLD),
            unfocused: none.add_modifier(Modifier::DIM),
            keyword: none.add_modifier(Modifier::BOLD),
            string: none,
            comment: none.add_modifier(Modifier::DIM),
            number: none,
            type_name: none,
            function: none,
            macro_name: none,
            attribute: none.add_modifier(Modifier::DIM),
        }
    }

    pub fn colored() -> Self {
        let fg = |c: Color| Style::new().fg(c);
        Self {
            path: fg(Color::Magenta).add_modifier(Modifier::BOLD),
            hit: fg(Color::Red).add_modifier(Modifier::BOLD),
            symbol: fg(Color::Cyan),
            declaration: fg(Color::Cyan).add_modifier(Modifier::BOLD),
            import: Style::new().add_modifier(Modifier::DIM),
            address: Style::new().add_modifier(Modifier::BOLD),
            reexport: fg(Color::Magenta),
            tick: fg(Color::Yellow).add_modifier(Modifier::BOLD),
            cursor: Style::new().bg(Color::DarkGray),
            added: fg(Color::Green),
            removed: fg(Color::Red),
            hunk: fg(Color::Cyan),
            title: Style::new().add_modifier(Modifier::BOLD),
            dim: Style::new().add_modifier(Modifier::DIM),
            error: fg(Color::Red).add_modifier(Modifier::BOLD),
            warning: fg(Color::Yellow).add_modifier(Modifier::BOLD),
            key: fg(Color::Cyan).add_modifier(Modifier::BOLD),
            focused: fg(Color::Cyan),
            unfocused: Style::new().add_modifier(Modifier::DIM),
            keyword: fg(Color::Magenta),
            string: fg(Color::Green),
            comment: Style::new().add_modifier(Modifier::DIM),
            number: fg(Color::Yellow),
            type_name: fg(Color::Cyan),
            function: fg(Color::Blue),
            macro_name: fg(Color::Magenta),
            attribute: fg(Color::Yellow).add_modifier(Modifier::DIM),
        }
    }

    pub fn highlight(&self, kind: HighlightKind) -> Style {
        match kind {
            HighlightKind::Keyword => self.keyword,
            HighlightKind::String => self.string,
            HighlightKind::Comment => self.comment,
            HighlightKind::Number => self.number,
            HighlightKind::Type => self.type_name,
            HighlightKind::Function => self.function,
            HighlightKind::Macro => self.macro_name,
            HighlightKind::Attribute => self.attribute,
        }
    }

    pub fn border(&self, focused: bool) -> Style {
        if focused {
            self.focused
        } else {
            self.unfocused
        }
    }

    /// The colour a semantic role takes.
    pub fn role(&self, role: display::Role) -> Style {
        match role {
            display::Role::Plain => Style::new(),
            display::Role::Dim => self.dim,
            display::Role::Title => self.title,
            display::Role::Strong => self.title,
            display::Role::Path => self.path,
            display::Role::LineNumber => self.dim,
            display::Role::Ordinal => self.dim,
            display::Role::Symbol => self.symbol,
            display::Role::Declaration => self.declaration,
            display::Role::Import => self.import,
            display::Role::Address => self.address,
            display::Role::Error => self.error,
            display::Role::Warning => self.warning,
            display::Role::Hint => self.key,
            display::Role::Hit => self.hit,
            display::Role::Added => self.added,
            display::Role::Removed => self.removed,
            display::Role::Hunk => self.hunk,
            display::Role::Mark(mark) => self.mark(mark),
        }
    }

    /// The colour a mark of the vocabulary takes, mirroring the CLI palette.
    pub fn mark(&self, mark: Mark) -> Style {
        match mark {
            Mark::Declaration | Mark::OtherDeclaration => self.declaration,
            Mark::Import | Mark::Path | Mark::Glob | Mark::External | Mark::ImportedBy => {
                self.import
            }
            Mark::ReExport | Mark::Oracle => self.reexport,
            Mark::Safe => self.added,
            Mark::Unverified => self.warning,
            Mark::Other => self.error,
            Mark::Nothing | Mark::ByName | Mark::Unticked => self.dim,
            Mark::ByHand => self.warning,
            Mark::Structure | Mark::Rewrite => self.hunk,
            Mark::Ticked => self.tick,
            Mark::Undo => self.key,
        }
    }

    /// A mark's glyph and a space, coloured — how a row or a title starts.
    pub fn glyph(&self, mark: Mark) -> Span<'static> {
        Span::styled(format!("{} ", mark.glyph()), self.mark(mark))
    }
}
