//! Explicit capability mapping; schemas come from the engine catalog.
use crate::output::mcp::McpIssue;
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map, Value};
use vvv_engine::{Call, Command, SchemaContract, SchemaQuery};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ToolKind {
    DiscardPlan,
    ApplyPlan,
    ValidatePlan,
    InspectPlan,
    PrepareRename,
    Discover,
    Search,
    Relationships,
    Navigate,
    Context,
    Continue,
    Expand,
}
impl ToolKind {
    pub const ALL: [Self; 12] = [
        Self::DiscardPlan,
        Self::ApplyPlan,
        Self::ValidatePlan,
        Self::InspectPlan,
        Self::PrepareRename,
        Self::Discover,
        Self::Search,
        Self::Relationships,
        Self::Navigate,
        Self::Context,
        Self::Continue,
        Self::Expand,
    ];
    fn default_max_output_bytes(self) -> usize {
        if matches!(self, Self::Discover) {
            32_768
        } else {
            16_384
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::DiscardPlan => "vvv_discard_plan",
            Self::ApplyPlan => "vvv_apply_plan",
            Self::ValidatePlan => "vvv_validate_plan",
            Self::InspectPlan => "vvv_inspect_plan",
            Self::PrepareRename => "vvv_prepare_rename",
            Self::Discover => "vvv_discover",
            Self::Search => "vvv_search",
            Self::Relationships => "vvv_relationships",
            Self::Navigate => "vvv_navigate",
            Self::Context => "vvv_context",
            Self::Continue => "vvv_continue",
            Self::Expand => "vvv_expand",
        }
    }
    fn command(self) -> Command {
        match self {
            Self::DiscardPlan => Command::DiscardPlan,
            Self::ApplyPlan => Command::ApplyPlan,
            Self::ValidatePlan => Command::ValidatePlan,
            Self::InspectPlan => Command::InspectPlan,
            Self::PrepareRename => Command::PrepareRename,
            Self::Discover => Command::Discover,
            Self::Search => Command::SearchPage,
            Self::Relationships => Command::Relationships,
            Self::Navigate => Command::Resolve,
            Self::Context => Command::ContextPage,
            Self::Continue => Command::Continue,
            Self::Expand => Command::Expand,
        }
    }
}
pub(crate) struct ToolEntry {
    pub tool: Tool,
    kind: ToolKind,
    input: jsonschema::Validator,
}
impl ToolEntry {
    pub fn new(kind: ToolKind) -> anyhow::Result<Self> {
        let mut input = SchemaQuery {
            for_command: Some(kind.command()),
            contract: SchemaContract::Arguments,
        }
        .execute()?
        .document;
        // Enum newtype variants use a root reference. Inline that object before
        // closing the adapter's argument surface or its properties get rejected.
        while let Some(Value::String(reference)) = input.remove("$ref") {
            let name = reference
                .strip_prefix("#/$defs/")
                .expect("local schema reference");
            let definition = input["$defs"][name]
                .as_object()
                .expect("object definition")
                .clone();
            input.extend(definition);
        }
        input.remove("$id"); // The adapter adds its own delivery policy field.
        if kind.command().is_read_only() {
            input["properties"]
                .as_object_mut()
                .expect("object schema")
                .insert(
                    "max_output_bytes".into(),
                    serde_json::json!({
                        "type": "integer",
                        "minimum": 1024,
                        "maximum": 1048576,
                        "default": kind.default_max_output_bytes()
                    }),
                );
        }
        input.insert("additionalProperties".into(), Value::Bool(false));
        let validator = jsonschema::validator_for(&Value::Object(input.clone()))?;
        let mut tool = Tool::new(kind.name(), kind.description(), input);
        tool.output_schema = Some(std::sync::Arc::new(
            SchemaQuery {
                for_command: Some(kind.command()),
                contract: SchemaContract::Response,
            }
            .execute()?
            .document,
        ));
        tool.annotations = Some(
            ToolAnnotations::new()
                .read_only(kind.command().is_read_only())
                .destructive(!kind.command().is_read_only())
                .open_world(matches!(kind, ToolKind::ValidatePlan)),
        );
        Ok(Self {
            tool,
            kind,
            input: validator,
        })
    }
    pub fn decode(&self, arguments: Option<Map<String, Value>>) -> Result<Call, McpIssue> {
        let mut arguments = arguments.unwrap_or_default();
        if !self.input.is_valid(&Value::Object(arguments.clone())) {
            return Err(McpIssue::InvalidArguments);
        }
        let max_output_bytes = arguments
            .remove("max_output_bytes")
            .and_then(|v| v.as_u64())
            .map(|value| value as usize)
            .unwrap_or_else(|| self.kind.default_max_output_bytes());
        arguments.insert(
            "command".into(),
            Value::String(self.kind.command().as_str().into()),
        );
        let request = serde_json::from_value(Value::Object(arguments))
            .map_err(|_| McpIssue::InvalidArguments)?;
        Ok(Call {
            id: None,
            max_output_bytes: self
                .kind
                .command()
                .is_read_only()
                .then_some(max_output_bytes),
            request,
        })
    }
}
