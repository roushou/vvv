use std::path::Path;

use clap::ColorChoice;

use vvv_engine::{Engine, Request, Workspace};
use vvv_rs::Builtins;

use crate::output::{OutputFormat, Reporter};

/// Everything a command needs beyond its own arguments.
pub struct Context {
    engine: Engine,
    format: OutputFormat,
    color: ColorChoice,
    verbose: bool,
    diff: bool,
}

impl Context {
    pub fn open(
        root: &Path,
        format: OutputFormat,
        color: ColorChoice,
        verbose: bool,
        diff: bool,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            engine: Engine::new(Workspace::disk(root)?, Builtins::registry()),
            format,
            color,
            verbose,
            diff,
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Run a request and report its answer.
    pub fn run(&self, request: Request) -> anyhow::Result<()> {
        let answer = self.engine.run(request)?;
        self.reporter().report(&answer)
    }

    #[cfg(feature = "tui")]
    pub fn color(&self) -> ColorChoice {
        self.color
    }

    pub fn reporter(&self) -> Box<dyn Reporter> {
        self.format.reporter(self.color, self.verbose, self.diff)
    }
}
