use std::path::PathBuf;

use clap::Args;

use crate::context::Context;
use vvv_engine::{DepsQuery, Request};

/// What a file imports, and which files import it
#[derive(Debug, Args)]
pub struct DepsCmd {
    /// The file to ask about
    pub path: PathBuf,
}

impl DepsCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::Deps(DepsQuery {
            path: self.path.clone().into(),
        }))
    }
}
