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
        let (path, position) = parse_location(&self.location)?;
        ctx.run(Request::Explain(ExplainQuery {
            path: path.clone().into(),
            position,
        }))
    }
}

/// `src/a.rs:12:4` → the path and a zero-based position.
fn parse_location(text: &str) -> anyhow::Result<(PathBuf, Position)> {
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
    Ok((
        PathBuf::from(path),
        Position::new(one_based(line, "line")?, one_based(column, "column")?),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_are_one_based_and_the_column_is_optional() {
        assert_eq!(
            parse_location("src/a.rs:12:4").unwrap(),
            (PathBuf::from("src/a.rs"), Position::new(11, 3))
        );
        assert_eq!(
            parse_location("src/a.rs:12").unwrap(),
            (PathBuf::from("src/a.rs"), Position::new(11, 0))
        );
        assert!(parse_location("src/a.rs").is_err());
        assert!(parse_location("src/a.rs:0").is_err());
    }
}
