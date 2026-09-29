use super::explain::Location;
use crate::context::Context;
use clap::{Args, ValueEnum};
use vvv_engine::{ContextBudget, ContextQuery, NavigationQuery, Request};

/// Gather bounded source context and directly related declarations
#[derive(Debug, Args)]
pub struct ContextCmd {
    /// `path:line[:column]`, 1-based as editors show them
    pub location: String,
    /// Source detail to retrieve for each declaration
    #[arg(long, value_enum, default_value_t = Detail::Body)]
    pub detail: Detail,
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
    /// Include the enclosing declaration at the requested detail
    #[arg(long)]
    pub include_enclosing: bool,
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
        query.detail = match self.detail {
            Detail::Body => vvv_engine::ContextDetail::Body,
            Detail::Signature => vvv_engine::ContextDetail::Signature,
        };
        query.references = self.references;
        query.include_enclosing = self.include_enclosing;
        ctx.run(Request::Context(query))
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Detail {
    Body,
    Signature,
}
