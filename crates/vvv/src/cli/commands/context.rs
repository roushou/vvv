use super::explain::Location;
use crate::context::Context;
use clap::Args;
use vvv_engine::{ContextBudget, ContextQuery, NavigationQuery, Request};

/// Gather bounded source context and directly related declarations
#[derive(Debug, Args)]
pub struct ContextCmd {
    /// `path:line[:column]`, 1-based as editors show them
    pub location: String,
    #[arg(long)]
    pub select: Option<String>,
    /// Maximum compact JSON result bytes (excluding the response envelope)
    #[arg(long, default_value_t = ContextBudget::DEFAULT_BYTES)]
    pub max_bytes: usize,
    #[arg(long, default_value_t = ContextBudget::DEFAULT_ITEMS)]
    pub max_items: usize,
    #[arg(long, default_value_t = ContextBudget::DEFAULT_LOOKUPS)]
    pub max_lookups: usize,
    #[arg(long, default_value_t = ContextBudget::DEFAULT_FILES)]
    pub max_files: usize,
    /// Also scan for incoming references with the same spelling
    #[arg(long)]
    pub references: bool,
}
impl ContextCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let Location { path, position } = self.location.parse()?;
        let mut query = ContextQuery::new(NavigationQuery::at(path, position).origin);
        query.selection = crate::cli::select::Select::new(self.select.as_slice()).selection()?;
        query.budget = ContextBudget {
            max_bytes: self.max_bytes,
            max_items: self.max_items,
            max_lookups: self.max_lookups,
            max_files: self.max_files,
        };
        query.references = self.references;
        ctx.run(Request::Context(query))
    }
}
