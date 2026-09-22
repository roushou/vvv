use std::path::PathBuf;

use clap::Args;
use vvv_engine::{MoveIntent, MoveSymbolIntent, Request};

use crate::context::Context;

/// Move a file (or, with --symbol, one declaration) and rewrite every reference to it. Previews by default
#[derive(Debug, Args)]
pub struct MoveCmd {
    /// File to move; with --symbol, the file declaring the symbol
    pub from: PathBuf,

    /// Destination path (a new name is allowed); with --symbol, an existing file of the same language
    pub to: PathBuf,

    /// Move this declaration from FROM to TO instead of the file, with what belongs to it
    #[arg(long, value_name = "NAME")]
    pub symbol: Option<String>,

    /// Write the changes instead of previewing them
    #[arg(long)]
    pub apply: bool,
}

impl MoveCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        if let Some(name) = self.symbol {
            let intent = MoveSymbolIntent::new(name, self.from, self.to);
            return ctx.run(Request::MoveSymbol {
                intent,
                apply: self.apply,
            });
        }
        ctx.run(Request::Move {
            intent: MoveIntent::new(self.from, self.to),
            apply: self.apply,
        })
    }
}
