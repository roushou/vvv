//! File preview with syntax colouring, loaded without acquiring the graph.
use crate::EngineError;
use serde::{Deserialize, Serialize};
use vvv_core::{Highlight, RelPath};

/// One file as it is now, with its syntax colouring: what a picker shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileQuery {
    pub path: RelPath,
}

/// `Engine::file`: one file as it is, with its syntax colouring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub path: RelPath,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<Highlight>,
}

/// One file as it is now, coloured by its language when one claims it.
impl FileQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<File, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine.workspace(), engine.languages())
    }

    pub(crate) fn execute_in(
        self,
        workspace: &crate::Workspace,
        languages: &vvv_core::LanguageRegistry,
    ) -> Result<File, EngineError> {
        let path = self.path.as_path();
        let file = workspace.load(path)?;
        let highlights = match languages.for_path(path) {
            Some(language) => {
                language
                    .highlights(file.text())
                    .map_err(|source| EngineError::Search {
                        path: path.into(),
                        source,
                    })?
            }
            None => Vec::new(),
        };
        Ok(File {
            path: file.path().into(),
            text: file.text().to_owned(),
            highlights,
        })
    }
}
