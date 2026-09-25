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
    bindings: &[Keybinding {
        triggers: &[Trigger::Key(Key::ctrl('c'))],
        dispatch: Run(A::Quit),
        when: When::Always,
        legend: Legend {
            bar: None,
            help: "quit",
        },
    }],
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
                bar: None,
                help: "next / previous panel",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::back_tab())],
            dispatch: Run(A::FocusPrev),
            when: When::Always,
            legend: Legend {
                bar: None,
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
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::char('j'))],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: bar("j/k", "move"),
                help: "move",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::char('k'))],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: bar("j/k", "move"),
                help: "move",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_down()), Trigger::Key(Key::page_up())],
            dispatch: Run(A::Page(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "page",
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
                bar: bar("?", "keys"),
                help: "this list",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('v'))],
            dispatch: Run(A::View),
            when: When::Always,
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
            when: When::Always,
            legend: Legend {
                bar: bar("v", "view"),
                help: "compact / detailed rows",
            },
        },
    ],
};
