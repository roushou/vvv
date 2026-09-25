use std::path::PathBuf;

use clap::Args;
use vvv_engine::{ExplainQuery, Position, Request};

use crate::context::Context;

/// What is at a position: the enclosing declaration, its address, who may see it, who imports it
#[derive(Debug, Args)]
pub struct ExplainCmd {
    /// `path:line[:column]`, 1-based as editors show them
    pub location: String,
}

impl ExplainCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let Location { path, position } = self.location.parse()?;
        ctx.run(Request::Explain(ExplainQuery {
            path: path.into(),
            position,
        }))
    }
}

/// A `path:line[:column]`, 1-based as editors show them.
#[derive(Debug, PartialEq, Eq)]
struct Location {
    path: PathBuf,
    position: Position,
}

impl std::str::FromStr for Location {
    type Err = anyhow::Error;

    /// `src/a.rs:12:4` → the path and a zero-based position.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.rsplitn(3, ':');
        let last = parts.next().unwrap_or_default();
        let (path, line, column) = match (parts.next(), parts.next()) {
            (Some(line), Some(path)) => (path, line, last),
            (Some(path), None) => (path, last, "1"),
            _ => anyhow::bail!("expected path:line[:column], got `{text}`"),
        };
        let one_based = |s: &str, what: &str| -> anyhow::Result<u32> {
            let n: u32 = s
                .parse()
                .map_err(|_| anyhow::anyhow!("{what} `{s}` is not a number in `{text}`"))?;
            n.checked_sub(1)
                .ok_or_else(|| anyhow::anyhow!("{what}s start at 1 in `{text}`"))
        };
        Ok(Self {
            path: PathBuf::from(path),
            position: Position::new(one_based(line, "line")?, one_based(column, "column")?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(text: &str) -> Location {
        text.parse().unwrap()
    }

    #[test]
    fn locations_are_one_based_and_the_column_is_optional() {
        assert_eq!(
            location("src/a.rs:12:4"),
            Location {
                path: PathBuf::from("src/a.rs"),
                position: Position::new(11, 3),
            }
        );
        assert_eq!(
            location("src/a.rs:12"),
            Location {
                path: PathBuf::from("src/a.rs"),
                position: Position::new(11, 0),
            }
        );
        assert!("src/a.rs".parse::<Location>().is_err());
        assert!("src/a.rs:0".parse::<Location>().is_err());
    }
}
