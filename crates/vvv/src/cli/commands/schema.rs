use clap::Args;
use vvv_engine::{Command, Request, SchemaContract, SchemaQuery};

use crate::context::Context;

/// Retrieve an offline JSON Schema for a command or session envelope
#[derive(Debug, Args)]
pub struct SchemaCmd {
    /// Command name; omit for call/reply session schemas
    pub for_command: Option<Command>,
    /// arguments, request, result, response, call, or reply
    #[arg(long, default_value = "arguments")]
    pub contract: SchemaContract,
}

impl SchemaCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        ctx.run(Request::Schema(SchemaQuery {
            for_command: self.for_command,
            contract: self.contract,
        }))
    }
}
