//! Capabilities exposed by this build, available without scanning the workspace.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryQuery {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    pub schema: u32,
    pub languages: Vec<crate::LanguageId>,
    pub commands: Vec<Capability>,
    pub context_defaults: crate::ContextBudget,
    pub context_maximum: crate::ContextBudget,
    pub min_output_bytes: usize,
    pub max_output_bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub command: String,
    /// Whether this command is always read-only, including every supported option.
    pub read_only: bool,
    pub parameters: Vec<String>,
}
impl DiscoveryQuery {
    pub fn execute(self, engine: &crate::Engine) -> Discovery {
        let entries: &[(&str, bool, &[&str])] = &[
            ("discover", true, &[]),
            (
                "context",
                true,
                &["origin", "selection", "budget", "references"],
            ),
            ("navigate", true, &["origin", "selection"]),
            (
                "search",
                true,
                &["pattern", "kind", "symbol", "name", "language"],
            ),
            ("outline", true, &["path"]),
            (
                "references",
                true,
                &["name", "symbol", "language", "declared_in"],
            ),
            ("where", true, &["name", "from"]),
            ("deps", true, &["path"]),
            ("explain", true, &["path", "position"]),
            ("surface", true, &["package"]),
            ("impact", true, &["name", "declared_in"]),
            ("dead", true, &["language"]),
            ("imports", true, &["path"]),
            ("file", true, &["path"]),
            (
                "rewrite",
                false,
                &["query", "template", "selection", "apply"],
            ),
            (
                "rename",
                false,
                &[
                    "name",
                    "to",
                    "symbol",
                    "language",
                    "declared_in",
                    "selection",
                    "apply",
                ],
            ),
            ("move", false, &["from", "to", "apply"]),
            ("move_symbol", false, &["name", "from", "to", "apply"]),
            ("batch", false, &["intents", "apply"]),
            ("history", true, &[]),
            ("undo", false, &[]),
        ];
        Discovery {
            schema: crate::protocol::SCHEMA,
            languages: engine.language_ids(),
            commands: entries
                .iter()
                .map(|(command, read_only, parameters)| Capability {
                    command: (*command).into(),
                    read_only: *read_only,
                    parameters: parameters.iter().map(|s| (*s).into()).collect(),
                })
                .collect(),
            context_defaults: crate::ContextBudget::default(),
            context_maximum: crate::ContextBudget {
                max_bytes: 1_048_576,
                max_items: 64,
                max_lookups: 512,
                max_files: 1024,
            },
            min_output_bytes: 1024,
            max_output_bytes: 1_048_576,
        }
    }
}
impl crate::report::Document {
    pub(crate) fn discovery(reply: &Discovery) -> Self {
        use crate::protocol::display::{Line, Role};
        let mut doc = Self::new();
        doc.body([Line::of(Role::Strong, "Available commands")]);
        for capability in &reply.commands {
            doc.body([Line::of(Role::Plain, &capability.command).and(
                Role::Dim,
                if capability.read_only {
                    " (read-only)"
                } else {
                    " (can write)"
                },
            )]);
        }
        doc
    }
}
