//! The keymap vocabulary: [`Key`] is a press, [`Trigger`] is what a
//! [`Keybinding`] listens for, [`Dispatch`] is what it does, and a
//! [`Layer`] is a named set of bindings. Pure — no terminal library and no
//! action type; the concrete layers live with the views and in
//! [`crate::screen::defaults`].

mod input;
mod keys;

pub use input::key as from_event;
pub use keys::Key;

/// What a binding listens for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Exactly this key.
    Key(Key),
    /// Any key that types a plain character.
    Text,
    /// Whatever is pressed.
    Any,
}

impl Trigger {
    fn matches(self, key: Key) -> bool {
        match self {
            Self::Key(bound) => bound == key,
            Self::Text => key.text().is_some(),
            Self::Any => true,
        }
    }

    /// How the trigger is written in the help and the status bar.
    pub fn spell(self) -> String {
        match self {
            Self::Key(key) => key.spell(),
            Self::Text => "typing".to_owned(),
            Self::Any => "any key".to_owned(),
        }
    }
}

/// What pressing a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch<A> {
    /// Run this action.
    Run(A),
    /// Put the typed character into the focused input.
    Type,
}

/// The status bar's entry for a binding: the keys as a short label and the
/// word after them. An empty word means the model supplies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bar {
    pub keys: &'static str,
    pub word: &'static str,
}

/// How a binding is named to a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Legend {
    /// The status bar's entry, when the binding has one.
    pub bar: Option<Bar>,
    /// The help overlay's words.
    pub help: &'static str,
}

/// One row of a keymap: the keys that fire it, what they do, when, and how
/// the row reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keybinding<A> {
    pub triggers: &'static [Trigger],
    pub dispatch: Dispatch<A>,
    pub when: When,
    pub legend: Legend,
}

impl<A: Copy> Keybinding<A> {
    /// Every key of the row spelled, for the help.
    pub fn spelled(&self) -> String {
        let mut keys: Vec<String> = self.triggers.iter().map(|t| t.spell()).collect();
        keys.dedup();
        keys.join(" ")
    }
}

/// A condition on the model a binding needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Always,
    /// The search query holds nothing.
    QueryEmpty,
    /// The search query holds something.
    QueryNotEmpty,
}

/// A named set of bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer<A: 'static> {
    pub name: &'static str,
    pub bindings: &'static [Keybinding<A>],
}

/// A line of the help and the status bar: consecutive bindings that share a
/// legend read as one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row<'a, A> {
    pub spelled: String,
    pub legend: Legend,
    pub binding: &'a Keybinding<A>,
}

impl<A: Copy> Layer<A> {
    /// What `key` does here, given which conditions hold: the first binding
    /// that matches.
    pub fn resolve(&self, key: Key, holds: impl Fn(When) -> bool) -> Option<Dispatch<A>> {
        self.bindings
            .iter()
            .filter(|b| holds(b.when))
            .find(|b| b.triggers.iter().any(|t| t.matches(key)))
            .map(|b| b.dispatch)
    }

    /// The bindings gathered into display rows: a run that shares a legend
    /// collapses into one, its keys joined.
    pub fn rows(&self) -> Vec<Row<'static, A>> {
        let mut rows: Vec<Row<'static, A>> = Vec::new();
        for binding in self.bindings {
            match rows.last_mut() {
                Some(row) if row.legend == binding.legend => {
                    let spelled = binding.spelled();
                    if !row.spelled.is_empty() && !spelled.is_empty() {
                        row.spelled.push(' ');
                    }
                    row.spelled.push_str(&spelled);
                }
                _ => rows.push(Row {
                    spelled: binding.spelled(),
                    legend: binding.legend,
                    binding,
                }),
            }
        }
        rows
    }
}
