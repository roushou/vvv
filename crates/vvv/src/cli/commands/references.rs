use std::path::PathBuf;

use clap::Args;
use vvv_engine::{LanguageId, ReferencesQuery, Request, SymbolKind};

use crate::context::Context;

/// Every identifier spelling a name, judged against the declaration it belongs to
#[derive(Debug, Args)]
pub struct ReferencesCmd {
    /// The declaration's name
    pub name: String,

    /// Only declarations of this kind (function, struct, class, ...)
    #[arg(short, long, value_name = "KIND")]
    pub symbol: Option<SymbolKind>,

    /// Only in files of this language
    #[arg(short, long)]
    pub lang: Option<String>,

    /// The file declaring the one meant, when several share the name
    #[arg(long = "in", value_name = "FILE")]
    pub declared_in: Option<PathBuf>,
}

impl ReferencesCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let query = ReferencesQuery {
            name: self.name,
            symbol: self.symbol,
            language: self.lang.map(LanguageId::from),
            declared_in: self.declared_in.clone().map(Into::into),
        };
        ctx.run(Request::References(query))
    }
}
