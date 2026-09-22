//! The one place a terminal key becomes a [`Key`]: crossterm's event model
//! translated into the keymap's terminal-independent one.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::keys::{Code, Key, Modifiers};

/// The keymap's key for a terminal event, or `None` for a key the picker
/// does not bind (function keys and the like).
pub fn key(event: KeyEvent) -> Option<Key> {
    let code = match event.code {
        KeyCode::Char(c) => Code::Char(c),
        KeyCode::Enter => Code::Enter,
        KeyCode::Esc => Code::Esc,
        KeyCode::Tab => Code::Tab,
        KeyCode::BackTab => Code::BackTab,
        KeyCode::Backspace => Code::Backspace,
        KeyCode::Delete => Code::Delete,
        KeyCode::Up => Code::Up,
        KeyCode::Down => Code::Down,
        KeyCode::Left => Code::Left,
        KeyCode::Right => Code::Right,
        KeyCode::Home => Code::Home,
        KeyCode::End => Code::End,
        KeyCode::PageUp => Code::PageUp,
        KeyCode::PageDown => Code::PageDown,
        _ => return None,
    };
    // Shift is folded into the character itself, so only ctrl and alt are
    // tracked; a shifted `M` is `Char('M')` with no modifier.
    let mut modifiers = Modifiers::NONE;
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers = modifiers.union(Modifiers::CTRL);
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        modifiers = modifiers.union(Modifiers::ALT);
    }
    Some(Key::new(code, modifiers))
}
