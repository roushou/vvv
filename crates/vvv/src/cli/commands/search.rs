use clap::Args;
use vvv_engine::{LanguageId, Query, Request, SymbolKind};

use crate::context::Context;

/// Find code by structure (pattern, node kind) or by declaration (symbol kind, name)
#[derive(Debug, Args)]
pub struct SearchCmd {
    /// Pattern with meta-variables. Must be a complete node, e.g. 'fn $NAME($$$ARGS) { $$$ }'
    pub pattern: Option<String>,

    /// Tree-sitter node kind, e.g. function_item, struct_item, interface_declaration
    #[arg(short, long)]
    pub kind: Option<String>,

    /// Only declarations of this kind (function, method, struct, class, enum, ...)
    #[arg(short, long, value_name = "KIND")]
    pub symbol: Option<SymbolKind>,

    /// Only declarations with exactly this name
    #[arg(short, long)]
    pub name: Option<String>,

    /// Only search files of this language (rust, typescript, tsx)
    #[arg(short, long)]
    pub lang: Option<String>,
}

impl SearchCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let query = Query::builder()
            .pattern(self.pattern)
            .kind(self.kind)
            .symbol(self.symbol)
            .name(self.name)
            .language(self.lang.map(LanguageId::from))
            .build()?;
        ctx.run(Request::Search(query))
    }
}
