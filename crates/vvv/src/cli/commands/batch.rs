use std::io::Read;
use std::path::PathBuf;

use clap::Args;
use vvv_engine::{BatchIntent, Intent, Request};

use crate::context::Context;

/// Plan several commands as one, each against what the previous leaves, and apply them together
#[derive(Debug, Args)]
pub struct BatchCmd {
    /// A JSON array of intents (the `intent` objects `--json` prints), or `-` for stdin
    pub intents: PathBuf,

    /// Write the changes instead of previewing them
    #[arg(long)]
    pub apply: bool,
}

impl BatchCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let text = if self.intents.as_os_str() == "-" {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            text
        } else {
            std::fs::read_to_string(&self.intents)?
        };
        let intents: Vec<Intent> = serde_json::from_str(&text)?;
        ctx.run(Request::Batch {
            intent: BatchIntent::new(intents),
            apply: self.apply,
        })
    }
}
