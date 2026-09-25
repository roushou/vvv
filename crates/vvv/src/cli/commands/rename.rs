use clap::Args;
use vvv_engine::{LanguageId, RenameIntent, Request, SymbolKind};

use crate::{cli::select::Select, context::Context};

/// Rename a declaration and every identifier spelling its name. Previews by default
#[derive(Debug, Args)]
pub struct RenameCmd {
    /// Current name of the symbol
    pub name: String,

    /// New name
    pub to: String,

    /// Require the declaration to be of this kind (function, struct, class, ...)
    #[arg(short, long, value_name = "KIND")]
    pub symbol: Option<SymbolKind>,

    /// Only consider this language (rust, typescript, tsx)
    #[arg(short, long)]
    pub lang: Option<String>,

    /// The file declaring the symbol meant, when several share the name
    #[arg(long = "in", value_name = "FILE")]
    pub declared_in: Option<std::path::PathBuf>,

    /// Only rename these occurrences: row numbers or ranges from a preview
    /// (`3,27-33`), or ids from `--json`; comma-separated
    #[arg(long, value_delimiter = ',', value_name = "ROWS|IDS")]
    pub select: Vec<String>,

    /// Write the changes instead of previewing them
    #[arg(long)]
    pub apply: bool,
}

impl RenameCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let mut intent = RenameIntent::new(self.name, self.to);
        intent.symbol = self.symbol;
        intent.language = self.lang.map(LanguageId::from);
        intent.declared_in = self.declared_in.clone().map(Into::into);
        intent.selection = Select::new(&self.select).selection()?;

        ctx.run(Request::Rename {
            intent,
            apply: self.apply,
        })
    }
}
