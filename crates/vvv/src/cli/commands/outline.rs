use std::path::PathBuf;

use clap::Args;

use crate::context::Context;
use vvv_engine::{OutlineQuery, Request};

/// List what a file declares, in order, with where each item is reached from
#[derive(Debug, Args)]
pub struct OutlineCmd {
    /// The file to outline
    pub path: PathBuf,
}

impl OutlineCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::Outline(OutlineQuery {
            path: self.path.clone().into(),
        }))
    }
}
