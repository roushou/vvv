//! How results reach the user. The engine returns values; a [`Reporter`]
//! decides how they look. The human reporter composes a document and prints
//! it; the JSON reporter serializes the answer.

mod diagnosis;
#[cfg(test)]
pub(crate) mod fixtures;
mod human;
mod json;
mod render;

use clap::ColorChoice;
use vvv_engine::protocol::Answer;

pub use diagnosis::Diagnose;
pub use human::HumanReporter;
pub use json::JsonReporter;
#[cfg(feature = "tui")]
pub use render::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

impl OutputFormat {
    pub fn reporter(self, color: ColorChoice, verbose: bool, diff: bool) -> Box<dyn Reporter> {
        match self {
            OutputFormat::Human => {
                Box::new(HumanReporter::stdio(color).verbose(verbose).diff(diff))
            }
            OutputFormat::Json => Box::new(JsonReporter::stdio()),
        }
    }
}

pub trait Reporter {
    /// Render one answer — the JSON envelope, or the human view.
    fn report(&mut self, answer: &Answer) -> anyhow::Result<()>;
    fn error(&mut self, error: &anyhow::Error);
}
