use clap::Args;

use crate::context::Context;
use vvv_engine::{DeadQuery, LanguageId, Request};

/// Declarations nothing in the workspace refers to, with how many tokens might
#[derive(Debug, Args)]
pub struct DeadCmd {
    /// Only this language (rust, typescript, tsx)
    #[arg(short, long)]
    pub lang: Option<String>,
}

impl DeadCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::Dead(DeadQuery {
            language: self.lang.map(LanguageId::from),
        }))
    }
}
