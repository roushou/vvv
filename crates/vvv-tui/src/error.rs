//! What `Tui::run` can fail with.

/// A session ended early: the terminal, the engine, or the editor failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("terminal: {0}")]
    Terminal(#[from] std::io::Error),
    #[error(transparent)]
    Engine(#[from] vvv_engine::EngineError),
    #[error("could not run `{command}`: {source}")]
    Editor {
        command: String,
        #[source]
        source: std::io::Error,
    },
}
