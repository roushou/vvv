use crate::{context::Context, mcp::McpSession};
use clap::Args;

/// Serve navigation, reviewed-plan, and validation MCP tools over stdio for this workspace
#[derive(Debug, Args)]
pub struct McpCmd;
impl McpCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        McpSession::run(
            ctx.engine()
                .clone()
                .with_retention(vvv_engine::Retention::session()),
        )
    }
}
