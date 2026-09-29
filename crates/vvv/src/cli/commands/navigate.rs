use super::explain::Location;
use crate::context::Context;
use clap::Args;
use vvv_engine::{NavigationQuery, Request};

/// Follow an identifier to its definition, with the captured definition source
#[derive(Debug, Args)]
pub struct NavigateCmd {
    /// `path:line[:column]`, 1-based as editors show them
    pub location: String,
    /// Choose one candidate by its 1-based row number or match id
    #[arg(long, value_name = "ROW_OR_ID")]
    pub select: Option<String>,
    /// Return a compact target and evidence without full-file preview data
    #[arg(long)]
    pub compact: bool,
}

impl NavigateCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let Location { path, position } = self.location.parse()?;
        let selection = crate::cli::select::Select::new(self.select.as_slice()).selection()?;
        let query = NavigationQuery::at(path, position).select(selection);
        ctx.run(if self.compact {
            Request::Resolve(vvv_engine::ResolutionQuery(query))
        } else {
            Request::Navigate(query)
        })
    }
}
