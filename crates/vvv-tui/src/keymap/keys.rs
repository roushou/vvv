//! Terminal-independent key values: what a binding matches and a person
//! presses. Nothing here knows about a terminal library.

/// Modifiers held with a key. Shift is folded into the character itself
/// (`Char('M')`), so it is not tracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const CTRL: Self = Self(1 << 0);
    pub const ALT: Self = Self(1 << 1);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// A key without its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Code {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// A key press: a code and the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: Code,
    pub modifiers: Modifiers,
}

impl Key {
    pub const fn new(code: Code, modifiers: Modifiers) -> Self {
        Self { code, modifiers }
    }

    pub const fn char(c: char) -> Self {
        Self::new(Code::Char(c), Modifiers::NONE)
    }

    pub const fn ctrl(c: char) -> Self {
        Self::new(Code::Char(c), Modifiers::CTRL)
    }

    pub const fn code(code: Code) -> Self {
        Self::new(code, Modifiers::NONE)
    }

    pub const fn enter() -> Self {
        Self::code(Code::Enter)
    }

    pub const fn esc() -> Self {
        Self::code(Code::Esc)
    }

    pub const fn tab() -> Self {
        Self::code(Code::Tab)
    }

    pub const fn back_tab() -> Self {
        Self::code(Code::BackTab)
    }

    pub const fn backspace() -> Self {
        Self::code(Code::Backspace)
    }

    pub const fn up() -> Self {
        Self::code(Code::Up)
    }

    pub const fn down() -> Self {
        Self::code(Code::Down)
    }

    pub const fn left() -> Self {
        Self::code(Code::Left)
    }

    pub const fn right() -> Self {
        Self::code(Code::Right)
    }

    pub const fn home() -> Self {
        Self::code(Code::Home)
    }

    pub const fn end() -> Self {
        Self::code(Code::End)
    }

    pub const fn page_up() -> Self {
        Self::code(Code::PageUp)
    }

    pub const fn page_down() -> Self {
        Self::code(Code::PageDown)
    }

    /// The character this key types, if it is a plain character: what goes
    /// into an input.
    pub fn text(&self) -> Option<char> {
        match self.code {
            Code::Char(c) if !self.modifiers.contains(Modifiers::CTRL) => Some(c),
            _ => None,
        }
    }

    /// How the key is written in the help and the status bar.
    pub fn spell(&self) -> String {
        if let Code::Char(c) = self.code {
            if self.modifiers.contains(Modifiers::CTRL) {
                return format!("ctrl+{c}");
            }
            if self.modifiers.contains(Modifiers::ALT) {
                return format!("alt+{c}");
            }
        }
        match self.code {
            Code::Char(' ') => "space".to_owned(),
            Code::Char(c) => c.to_string(),
            Code::Enter => "⏎".to_owned(),
            Code::Esc => "esc".to_owned(),
            Code::Tab => "tab".to_owned(),
            Code::BackTab => "s-tab".to_owned(),
            Code::Backspace => "bksp".to_owned(),
            Code::Delete => "del".to_owned(),
            Code::Up => "↑".to_owned(),
            Code::Down => "↓".to_owned(),
            Code::Left => "←".to_owned(),
            Code::Right => "→".to_owned(),
            Code::PageUp => "pgup".to_owned(),
            Code::PageDown => "pgdn".to_owned(),
            Code::Home => "home".to_owned(),
            Code::End => "end".to_owned(),
        }
    }
}
