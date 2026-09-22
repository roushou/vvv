use clap::Args;

use crate::context::Context;
use vvv_engine::Request;

/// List applies that `vvv undo` can still reverse
#[derive(Debug, Args)]
pub struct HistoryCmd {}

impl HistoryCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::History)
    }
}
