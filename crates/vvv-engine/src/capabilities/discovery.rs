//! Capabilities exposed by this build, available without scanning the workspace.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiscoveryQuery {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Discovery {
    #[serde(default)]
    pub schemas_available: bool,
    pub validation_available: bool,
    pub validation_defaults: crate::ValidationBudget,
    pub schema: u32,
    pub languages: Vec<crate::LanguageId>,
    pub commands: Vec<Capability>,
    pub page_defaults: crate::PageBudget,
    pub page_maximum: crate::PageBudget,
    pub work_defaults: crate::WorkBudget,
    pub work_maximum: crate::WorkBudget,
    pub query_retention: crate::QueryLimits,
    pub plan_retention: crate::PlanLimits,
    pub context_defaults: crate::ContextBudget,
    pub context_maximum: crate::ContextBudget,
    pub min_output_bytes: usize,
    pub max_output_bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Capability {
    #[cfg(feature = "schema")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schemas: Option<crate::SchemaReferences>,
    pub command: String,
    /// Whether this command is always read-only, including every supported option.
    pub read_only: bool,
    pub parameters: Vec<String>,
}
impl DiscoveryQuery {
    pub fn execute(self, engine: &crate::Engine) -> Discovery {
        Discovery {
            schema: crate::protocol::SCHEMA,
            validation_available: crate::ValidatePlanQuery::available(engine),
            validation_defaults: crate::ValidationBudget::default(),
            languages: engine.language_ids(),
            schemas_available: cfg!(feature = "schema"),
            commands: crate::Command::ALL
                .iter()
                .map(|&command| Capability {
                    command: command.as_str().into(),
                    read_only: command.is_read_only(),
                    parameters: command.parameters().iter().map(|p| (*p).into()).collect(),
                    #[cfg(feature = "schema")]
                    schemas: Some(crate::SchemaReferences::for_command(command)),
                })
                .collect(),
            page_defaults: crate::PageBudget::default(),
            page_maximum: crate::PageBudget::MAXIMUM,
            work_defaults: crate::WorkBudget::default(),
            work_maximum: crate::WorkBudget::MAXIMUM,
            query_retention: crate::QueryLimits::default(),
            plan_retention: crate::PlanLimits::default(),
            context_defaults: crate::ContextBudget::default(),
            context_maximum: crate::ContextBudget::MAXIMUM,
            min_output_bytes: crate::ContextBudget::MIN_BYTES,
            max_output_bytes: crate::ContextBudget::MAX_BYTES,
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
