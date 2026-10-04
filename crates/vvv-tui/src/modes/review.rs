//! Readiness shared by operation headers, apply hints and commit guards.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState<'a> {
    Input(&'a str),
    Planning,
    Applying,
    Failed(&'a str),
    Empty(&'a str),
    Ready,
}

impl<'a> ReviewState<'a> {
    pub fn message(self) -> &'a str {
        match self {
            Self::Input(text) | Self::Failed(text) | Self::Empty(text) => text,
            Self::Planning => "updating preview",
            Self::Applying => "applying",
            Self::Ready => "ready to apply",
        }
    }

    pub fn apply_hint(self) -> &'a str {
        match self {
            Self::Input(_) => "edit input",
            Self::Planning => "planning",
            Self::Applying => "applying",
            Self::Failed(_) => "fix error",
            Self::Empty(text) => text,
            Self::Ready => "apply",
        }
    }
}
