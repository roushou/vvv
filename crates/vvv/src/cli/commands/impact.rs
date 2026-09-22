use std::path::PathBuf;

use clap::Args;

use crate::context::Context;
use vvv_engine::{ImpactQuery, Request};

/// Who would feel a change to a declaration: the modules importing it, then the modules importing those, outward
#[derive(Debug, Args)]
pub struct ImpactCmd {
    /// The declaration's name
    pub name: String,

    /// The file declaring the symbol meant, when several share the name
    #[arg(long = "in", value_name = "FILE")]
    pub declared_in: Option<PathBuf>,
}

impl ImpactCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::Impact(ImpactQuery {
            name: self.name,
            declared_in: self.declared_in.clone().map(Into::into),
        }))
    }
}
