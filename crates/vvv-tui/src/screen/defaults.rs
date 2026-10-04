//! The bindings every view shares: the globals that are never shadowed, the
//! focus keys, and the defaults a panel of each kind answers.

use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};

use Action as A;
use Dispatch::Run;

/// A status-bar entry.
const fn bar(keys: &'static str, word: &'static str) -> Option<Bar> {
    Some(Bar { keys, word })
}

/// Checked before every layer, overlays included: quitting is not a view's
/// business.
pub static GLOBAL: Layer<Action> = Layer {
    name: "Everywhere",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::help())],
            dispatch: Run(A::Help),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "help for the focused pane; press again to return",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('c'))],
            dispatch: Run(A::Quit),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "quit",
            },
        },
    ],
};

/// Moving focus; every view that is not an overlay answers it.
pub static NAVIGATE: Layer<Action> = Layer {
    name: "Everywhere",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::tab())],
            dispatch: Run(A::FocusNext),
            when: When::Always,
            legend: Legend {
                bar: bar("tab", "panes"),
                help: "next / previous panel",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::back_tab())],
            dispatch: Run(A::FocusPrev),
            when: When::Always,
            legend: Legend {
                bar: bar("tab", "panes"),
                help: "next / previous panel",
            },
        },
    ],
};

/// Jumping to the n-th panel; a view whose focus is an input leaves it out,
/// because there the digits are text.
pub static DIGITS: Layer<Action> = Layer {
    name: "Everywhere",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::char('1'))],
            dispatch: Run(A::FocusNth(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "the n-th panel",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('2'))],
            dispatch: Run(A::FocusNth(2)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "the n-th panel",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('3'))],
            dispatch: Run(A::FocusNth(3)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "the n-th panel",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('4'))],
            dispatch: Run(A::FocusNth(4)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "the n-th panel",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('5'))],
            dispatch: Run(A::FocusNth(5)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "the n-th panel",
            },
        },
    ],
};

/// What any list answers.
pub static LIST: Layer<Action> = Layer {
    name: "Lists",
    bindings: &[
        Keybinding {
            triggers: &[
                Trigger::Key(Key::down()),
                Trigger::Key(Key::char('j')),
                Trigger::Key(Key::ctrl('n')),
            ],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: bar("j/k", "move"),
                help: "move",
            },
        },
        Keybinding {
            triggers: &[
                Trigger::Key(Key::up()),
                Trigger::Key(Key::char('k')),
                Trigger::Key(Key::ctrl('p')),
            ],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: bar("j/k", "move"),
                help: "move",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_down())],
            dispatch: Run(A::Page(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_up())],
            dispatch: Run(A::Page(-1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "previous page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::home()), Trigger::Key(Key::char('g'))],
            dispatch: Run(A::Top),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "first / last row",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::end()), Trigger::Key(Key::char('G'))],
            dispatch: Run(A::Bottom),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "first / last row",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('e'))],
            dispatch: Run(A::Edit),
            when: When::Always,
            legend: Legend {
                bar: bar("e", "editor"),
                help: "open $EDITOR at the row",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('?'))],
            dispatch: Run(A::Help),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "this list",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('v'))],
            dispatch: Run(A::View),
            when: When::ReportViewAvailable,
            legend: Legend {
                bar: bar("v", "view"),
                help: "compact / detailed rows",
            },
        },
    ],
};

/// What any text panel answers.
pub static TEXT: Layer<Action> = Layer {
    name: "Text panels",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::char('j'))],
            dispatch: Run(A::Scroll(1)),
            when: When::Always,
            legend: Legend {
                bar: bar("j/k", "scroll"),
                help: "scroll",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::char('k'))],
            dispatch: Run(A::Scroll(-1)),
            when: When::Always,
            legend: Legend {
                bar: bar("j/k", "scroll"),
                help: "scroll",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_down()), Trigger::Key(Key::char('d'))],
            dispatch: Run(A::Scroll(20)),
            when: When::Always,
            legend: Legend {
                bar: bar("d/u", "page"),
                help: "page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_up()), Trigger::Key(Key::char('u'))],
            dispatch: Run(A::Scroll(-20)),
            when: When::Always,
            legend: Legend {
                bar: bar("d/u", "page"),
                help: "page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::home()), Trigger::Key(Key::char('g'))],
            dispatch: Run(A::Top),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "top / bottom",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::end()), Trigger::Key(Key::char('G'))],
            dispatch: Run(A::Bottom),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "top / bottom",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('e'))],
            dispatch: Run(A::Edit),
            when: When::Always,
            legend: Legend {
                bar: bar("e", "editor"),
                help: "open $EDITOR at the row",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('?'))],
            dispatch: Run(A::Help),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "keys",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('v'))],
            dispatch: Run(A::View),
            when: When::ReportViewAvailable,
            legend: Legend {
                bar: bar("v", "view"),
                help: "compact / detailed rows",
            },
        },
    ],
};

/// Shared editing keys, active only for the focused input.
pub static INPUT: Layer<Action> = Layer {
    name: "Input editing",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::backspace())],
            dispatch: Run(A::Backspace),
            when: When::InputFocused,
            legend: Legend {
                bar: None,
                help: "delete previous grapheme",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('u'))],
            dispatch: Run(A::Clear),
            when: When::InputFocused,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+u",
                    word: "clear",
                }),
                help: "clear input",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::left())],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::Left)),
            when: When::InputFocused,
            legend: Legend {
                bar: Some(Bar {
                    keys: "←/→",
                    word: "caret",
                }),
                help: "move caret",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::right())],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::Right)),
            when: When::InputFocused,
            legend: Legend {
                bar: Some(Bar {
                    keys: "←/→",
                    word: "caret",
                }),
                help: "move caret",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::home())],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::Home)),
            when: When::InputFocused,
            legend: Legend {
                bar: None,
                help: "start of input",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::end())],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::End)),
            when: When::InputFocused,
            legend: Legend {
                bar: None,
                help: "end of input",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl_left())],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::WordLeft)),
            when: When::InputFocused,
            legend: Legend {
                bar: None,
                help: "previous word",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl_right())],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::WordRight)),
            when: When::InputFocused,
            legend: Legend {
                bar: None,
                help: "next word",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::delete())],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::Delete)),
            when: When::InputFocused,
            legend: Legend {
                bar: None,
                help: "delete next character",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl_delete())],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::WordDelete)),
            when: When::InputFocused,
            legend: Legend {
                bar: None,
                help: "delete next word",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl_backspace())],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::WordBackspace)),
            when: When::InputFocused,
            legend: Legend {
                bar: None,
                help: "delete previous word",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('w'))],
            dispatch: Run(A::InputEdit(crate::input::EditCommand::WordBackspace)),
            when: When::InputFocused,
            legend: Legend {
                bar: None,
                help: "delete previous word",
            },
        },
    ],
};

pub static RECOVERY: Layer<Action> = Layer {
    name: "Recovery",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::char('e'))],
            dispatch: Run(A::Edit),
            when: When::RecoveryFile,
            legend: Legend {
                bar: Some(Bar {
                    keys: "e",
                    word: "inspect file",
                }),
                help: "open the first remaining or unverified recovery file in the editor",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('r'))],
            dispatch: Run(A::Recover),
            when: When::Recoverable,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+r",
                    word: "",
                }),
                help: "refresh or rebuild the preview; applying remains a separate action",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('i'))],
            dispatch: Run(A::FocusNth(1)),
            when: When::Problem,
            legend: Legend {
                bar: Some(Bar {
                    keys: "i",
                    word: "edit input",
                }),
                help: "return to the retained input",
            },
        },
    ],
};

/// Source inspection shared by search and workspace previews.
pub const PREVIEW: Layer<Action> = Layer {
    name: "Preview inspection",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::char('/'))],
            dispatch: Run(A::InspectFind),
            when: When::PreviewInspectable,
            legend: Legend {
                bar: Some(Bar {
                    keys: "/",
                    word: "find",
                }),
                help: "find literal text in this preview",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char(':'))],
            dispatch: Run(A::InspectLine),
            when: When::PreviewInspectable,
            legend: Legend {
                bar: Some(Bar {
                    keys: ":",
                    word: "line",
                }),
                help: "go to an absolute file line in this preview",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('n'))],
            dispatch: Run(A::InspectNext(1)),
            when: When::PreviewInspectable,
            legend: Legend {
                bar: Some(Bar {
                    keys: "n",
                    word: "hit",
                }),
                help: "next / previous preview find hit; wraps",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('N'))],
            dispatch: Run(A::InspectNext(-1)),
            when: When::PreviewInspectable,
            legend: Legend {
                bar: None,
                help: "previous preview find hit; wraps",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::right()), Trigger::Key(Key::char('l'))],
            dispatch: Run(A::InspectHorizontal(8)),
            when: When::PreviewInspectable,
            legend: Legend {
                bar: Some(Bar {
                    keys: "→",
                    word: "columns",
                }),
                help: "scroll right by eight terminal columns",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::left()), Trigger::Key(Key::char('h'))],
            dispatch: Run(A::InspectHorizontal(-8)),
            when: When::PreviewInspectable,
            legend: Legend {
                bar: None,
                help: "scroll left by eight terminal columns",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('0'))],
            dispatch: Run(A::InspectStart),
            when: When::PreviewInspectable,
            legend: Legend {
                bar: None,
                help: "restore the first code column",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('z'))],
            dispatch: Run(A::ExpandPreview),
            when: When::PreviewInspectable,
            legend: Legend {
                bar: Some(Bar {
                    keys: "z",
                    word: "",
                }),
                help: "expand / restore this preview",
            },
        },
    ],
};
