use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// What a declaration declares, in vocabulary shared by every language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Class,
    Enum,
    Variant,
    Trait,
    Interface,
    TypeAlias,
    Const,
    Static,
    Variable,
    Field,
    Module,
    Macro,
    /// An `impl` block, named after the type it is for. Not a declaration a
    /// path can reach; it travels with its type when the type moves.
    Impl,
}

impl SymbolKind {
    pub const ALL: [SymbolKind; 16] = [
        Self::Function,
        Self::Method,
        Self::Struct,
        Self::Class,
        Self::Enum,
        Self::Variant,
        Self::Trait,
        Self::Interface,
        Self::TypeAlias,
        Self::Const,
        Self::Static,
        Self::Variable,
        Self::Field,
        Self::Module,
        Self::Macro,
        Self::Impl,
    ];

    /// Comma-separated list of every kind, for help and error messages.
    pub fn choices() -> String {
        Self::ALL.map(Self::as_str).join(", ")
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Class => "class",
            Self::Enum => "enum",
            Self::Variant => "variant",
            Self::Trait => "trait",
            Self::Interface => "interface",
            Self::TypeAlias => "type-alias",
            Self::Const => "const",
            Self::Static => "static",
            Self::Variable => "variable",
            Self::Field => "field",
            Self::Module => "module",
            Self::Macro => "macro",
            Self::Impl => "impl",
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown symbol kind `{0}`; expected one of {choices}", choices = SymbolKind::choices())]
pub struct UnknownSymbolKind(pub String);

impl FromStr for SymbolKind {
    type Err = UnknownSymbolKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| UnknownSymbolKind(s.to_owned()))
    }
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_text_and_serde() {
        for kind in SymbolKind::ALL {
            assert_eq!(kind.as_str().parse::<SymbolKind>(), Ok(kind));
        }
        assert_eq!(
            "bogus".parse::<SymbolKind>(),
            Err(UnknownSymbolKind("bogus".into()))
        );
    }
}
