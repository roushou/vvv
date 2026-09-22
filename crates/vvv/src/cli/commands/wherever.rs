use std::path::PathBuf;

use clap::Args;

use crate::context::Context;
use vvv_engine::{Request, WhereQuery};

/// Where a name is declared, and the import that reaches it from a file
#[derive(Debug, Args)]
pub struct WhereCmd {
    /// The declaration's name
    pub name: String,

    /// Spell the import as this file would write it
    #[arg(long, value_name = "FILE")]
    pub from: Option<PathBuf>,
}

impl WhereCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::Where(WhereQuery {
            name: self.name,
            from: self.from.clone().map(Into::into),
        }))
    }
}
