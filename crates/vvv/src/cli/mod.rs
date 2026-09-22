mod commands;
pub mod select;

#[cfg(feature = "tui")]
use std::io::IsTerminal;
use std::path::PathBuf;

use clap::{ColorChoice, CommandFactory, Parser, Subcommand};

use crate::context::Context;
use crate::output::OutputFormat;

#[derive(Debug, Parser)]
#[command(
    name = "vvv",
    version,
    about = "Language-aware refactoring for codebases",
    after_help = "Run without a command in a terminal to open the interactive picker."
)]
pub struct Cli {
    /// Workspace root
    #[arg(short = 'C', long, global = true, default_value = ".")]
    root: PathBuf,

    /// Emit machine-readable JSON instead of human output
    #[arg(long, global = true)]
    json: bool,

    /// When to use colours: auto (default, on for terminals), always, never.
    /// NO_COLOR is honoured
    #[arg(long, global = true, value_name = "WHEN", default_value_t = ColorChoice::Auto)]
    color: ColorChoice,

    /// Expand what the human view collapses: every ✓ row of a rename, not
    /// just per-file counts
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Print the full patch of a preview, not only its structural edits
    #[arg(long, global = true)]
    diff: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Search(commands::search::SearchCmd),
    Outline(commands::outline::OutlineCmd),
    References(commands::references::ReferencesCmd),
    #[command(name = "where")]
    Where(commands::wherever::WhereCmd),
    Deps(commands::deps::DepsCmd),
    Surface(commands::surface::SurfaceCmd),
    Impact(commands::impact::ImpactCmd),
    Dead(commands::dead::DeadCmd),
    Imports(commands::imports::ImportsCmd),
    Explain(commands::explain::ExplainCmd),
    Rewrite(commands::rewrite::RewriteCmd),
    Rename(commands::rename::RenameCmd),
    Move(commands::move_file::MoveCmd),
    Batch(commands::batch::BatchCmd),
    Undo(commands::undo::UndoCmd),
    History(commands::history::HistoryCmd),
    Serve(commands::serve::ServeCmd),
    /// Open the interactive picker: search, select, preview, apply, undo
    #[cfg(feature = "tui")]
    Ui,
}

impl Cli {
    pub fn parse() -> Self {
        <Self as Parser>::parse()
    }

    pub fn output_format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else {
            OutputFormat::Human
        }
    }

    pub fn color(&self) -> ColorChoice {
        self.color
    }

    /// The picker, with the CLI's colour policy and the user's editor.
    #[cfg(feature = "tui")]
    fn picker(ctx: &Context) -> anyhow::Result<()> {
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .ok();
        let color = crate::output::Palette::enabled(ctx.color(), std::io::stdout().is_terminal());
        vvv_tui::Tui::new(ctx.engine().clone())
            .editor(editor)
            .color(color)
            .run()?;
        Ok(())
    }

    pub fn run(self) -> anyhow::Result<()> {
        let ctx = Context::open(
            &self.root,
            self.output_format(),
            self.color,
            self.verbose,
            self.diff,
        )?;
        match self.command {
            Some(Commands::Search(cmd)) => cmd.run(&ctx),
            Some(Commands::Outline(cmd)) => cmd.run(&ctx),
            Some(Commands::References(cmd)) => cmd.run(&ctx),
            Some(Commands::Where(cmd)) => cmd.run(&ctx),
            Some(Commands::Deps(cmd)) => cmd.run(&ctx),
            Some(Commands::Surface(cmd)) => cmd.run(&ctx),
            Some(Commands::Impact(cmd)) => cmd.run(&ctx),
            Some(Commands::Dead(cmd)) => cmd.run(&ctx),
            Some(Commands::Imports(cmd)) => cmd.run(&ctx),
            Some(Commands::Explain(cmd)) => cmd.run(&ctx),
            Some(Commands::Rewrite(cmd)) => cmd.run(&ctx),
            Some(Commands::Rename(cmd)) => cmd.run(&ctx),
            Some(Commands::Move(cmd)) => cmd.run(&ctx),
            Some(Commands::Batch(cmd)) => cmd.run(&ctx),
            Some(Commands::Undo(cmd)) => cmd.run(&ctx),
            Some(Commands::History(cmd)) => cmd.run(&ctx),
            Some(Commands::Serve(cmd)) => cmd.run(&ctx),
            #[cfg(feature = "tui")]
            Some(Commands::Ui) => Self::picker(&ctx),
            #[cfg(feature = "tui")]
            None if std::io::stdout().is_terminal() && !self.json => Self::picker(&ctx),
            None => {
                Self::command().print_help()?;
                Ok(())
            }
        }
    }
}
