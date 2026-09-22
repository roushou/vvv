use std::path::PathBuf;

use clap::Args;

use crate::context::Context;
use vvv_engine::{ImportsQuery, Request};

/// Import statements worth a look: unresolved, unused, or naming what another already brings in
#[derive(Debug, Args)]
pub struct ImportsCmd {
    /// One file; every file when omitted
    pub path: Option<PathBuf>,
}

impl ImportsCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::Imports(ImportsQuery {
            path: self.path.clone().map(Into::into),
        }))
    }
}
