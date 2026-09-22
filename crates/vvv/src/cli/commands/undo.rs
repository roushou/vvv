use clap::Args;

use crate::context::Context;
use vvv_engine::Request;

/// Reverse the most recent --apply, if its files are untouched since
#[derive(Debug, Args)]
pub struct UndoCmd {}

impl UndoCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::Undo)
    }
}
