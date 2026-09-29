use super::explain::Location;
use crate::context::Context;
use clap::{Args, ValueEnum};
use vvv_engine::{
    NavigationQuery, RelationshipBudget, RelationshipKind, RelationshipsQuery, Request, SearchScope,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Kind {
    Callers,
    Callees,
    References,
}
impl From<Kind> for RelationshipKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Callers => Self::Callers,
            Kind::Callees => Self::Callees,
            Kind::References => Self::References,
        }
    }
}
/// Find call and reference sites with explicit resolution evidence
#[derive(Debug, Args)]
pub struct RelationshipsCmd {
    #[arg(value_enum)]
    pub kind: Kind,
    /// `path:line[:column]`, 1-based
    pub location: String,
    #[arg(long)]
    pub select: Option<String>,
    #[arg(long)]
    pub path: Vec<vvv_engine::RelPath>,
    #[arg(long)]
    pub package: Vec<String>,
    #[arg(long, default_value_t = RelationshipBudget::default().max_bytes)]
    pub max_bytes: usize,
    #[arg(long, default_value_t = RelationshipBudget::default().max_items)]
    pub max_items: usize,
    #[arg(long, default_value_t = RelationshipBudget::default().max_lookups)]
    pub max_lookups: usize,
    #[arg(long, default_value_t = RelationshipBudget::default().max_files)]
    pub max_files: usize,
}
impl RelationshipsCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let Location { path, position } = self.location.parse()?;
        let mut query =
            RelationshipsQuery::new(NavigationQuery::at(path, position).origin, self.kind.into());
        query.selection = crate::cli::select::Select::new(self.select.as_slice()).selection()?;
        query.scope = SearchScope {
            paths: self.path,
            packages: self.package,
        };
        query.budget = RelationshipBudget {
            max_bytes: self.max_bytes,
            max_items: self.max_items,
            max_lookups: self.max_lookups,
            max_files: self.max_files,
        };
        ctx.run(Request::Relationships(query))
    }
}
