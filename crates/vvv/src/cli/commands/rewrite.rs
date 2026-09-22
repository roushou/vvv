use vvv_engine::{LanguageId, Query, Request, RewriteIntent};

use crate::context::Context;

/// Replace matches of a pattern with a template. Previews by default
#[derive(Debug, clap::Args)]
pub struct RewriteCmd {
    /// Pattern with meta-variables. Must be a complete node, e.g. 'foo($$$ARGS)'
    pub pattern: String,

    /// Replacement reusing the pattern's meta-variables, e.g. 'bar($$$ARGS)'
    pub template: String,

    /// Tree-sitter node kind the match must have
    #[arg(short, long)]
    pub kind: Option<String>,

    /// Only rewrite files of this language (rust, typescript, tsx)
    #[arg(short, long)]
    pub lang: Option<String>,

    /// Only rewrite these matches: row numbers or ranges from `vvv search`
    /// (`3,27-33`), or ids from `--json`; comma-separated
    #[arg(short, long, value_delimiter = ',', value_name = "ROWS|IDS")]
    pub select: Vec<String>,

    /// Write the changes instead of previewing them
    #[arg(long)]
    pub apply: bool,
}

impl RewriteCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let query = Query::builder()
            .pattern(Some(self.pattern))
            .kind(self.kind)
            .language(self.lang.map(LanguageId::from))
            .build()?;
        let selection = crate::cli::select::parse(&self.select)?;
        let intent = RewriteIntent::new(query, self.template.as_str()).selecting(selection);

        ctx.run(Request::Rewrite {
            intent,
            apply: self.apply,
        })
    }
}
