use clap::Args;

use crate::context::Context;
use vvv_engine::{Request, SurfaceQuery};

/// What a package offers to everyone: its public declarations, where re-exports offer them, and who imports them
#[derive(Debug, Args)]
pub struct SurfaceCmd {
    /// The package, as paths name it (`vvv_core`); every package when omitted
    pub package: Option<String>,
}

impl SurfaceCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::Surface(SurfaceQuery {
            package: self.package,
        }))
    }
}
