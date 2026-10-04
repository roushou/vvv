//! Offline, generated JSON contracts. No workspace access or transport dependencies.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use schemars::{JsonSchema, generate::SchemaSettings};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Command, EngineError};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SchemaContract {
    Arguments,
    Request,
    Result,
    Response,
    Call,
    Reply,
}

impl SchemaContract {
    pub const COMMAND: &[Self] = &[Self::Arguments, Self::Request, Self::Result, Self::Response];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Arguments => "arguments",
            Self::Request => "request",
            Self::Result => "result",
            Self::Response => "response",
            Self::Call => "call",
            Self::Reply => "reply",
        }
    }
}

impl std::str::FromStr for SchemaContract {
    type Err = serde_json::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        serde_json::from_value(Value::String(value.into()))
    }
}

/// A command contract, or an aggregate session contract without `for_command`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SchemaQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_command: Option<Command>,
    pub contract: SchemaContract,
}

impl SchemaQuery {
    pub fn execute(self) -> Result<SchemaDocument, EngineError> {
        if self.for_command.is_some() != SchemaContract::COMMAND.contains(&self.contract) {
            return Err(EngineError::InvalidSchemaQuery);
        }
        Ok(SchemaDocument::catalog()[&self.artifact_name()].clone())
    }

    fn artifact_name(&self) -> String {
        match self.for_command {
            Some(command) => format!("{}.{}.json", command.as_str(), self.contract.as_str()),
            None => format!("{}.json", self.contract.as_str()),
        }
    }
}

/// Identifiers of bundled, locally retrievable command contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SchemaReferences {
    pub arguments: String,
    pub request: String,
    pub result: String,
    pub response: String,
}

impl SchemaReferences {
    pub(crate) fn for_command(command: Command) -> Self {
        let id = |contract| {
            SchemaDocument::catalog()[&SchemaQuery {
                for_command: Some(command),
                contract,
            }
            .artifact_name()]
                .id
                .clone()
        };
        Self {
            arguments: id(SchemaContract::Arguments),
            request: id(SchemaContract::Request),
            result: id(SchemaContract::Result),
            response: id(SchemaContract::Response),
        }
    }
}

/// A Draft 2020-12 schema, including all definitions needed to use it offline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SchemaDocument {
    pub id: String,
    pub document: Map<String, Value>,
}

impl SchemaDocument {
    /// Deterministic artifact filenames and contracts for this build. No I/O.
    pub fn catalog() -> &'static BTreeMap<String, Self> {
        static DOCUMENTS: OnceLock<BTreeMap<String, SchemaDocument>> = OnceLock::new();
        DOCUMENTS.get_or_init(|| {
            let request = Self::generate::<crate::Request>(false);
            let mut documents = BTreeMap::new();
            for &command in Command::ALL {
                for &contract in SchemaContract::COMMAND {
                    let mut document = match contract {
                        SchemaContract::Arguments | SchemaContract::Request => {
                            Self::request(&request, command, contract)
                        }
                        SchemaContract::Result => Self::result(command, false),
                        SchemaContract::Response => Self::result(command, true),
                        SchemaContract::Call | SchemaContract::Reply => unreachable!(),
                    };
                    document.insert(
                        "title".into(),
                        format!("{} {}", command.as_str(), contract.as_str()).into(),
                    );
                    let query = SchemaQuery {
                        for_command: Some(command),
                        contract,
                    };
                    documents.insert(query.artifact_name(), Self::new(&query, document));
                }
            }
            for (contract, document) in [
                (SchemaContract::Call, Self::generate::<crate::Call>(false)),
                (
                    SchemaContract::Reply,
                    Self::generate::<crate::Reply<crate::Answer>>(true),
                ),
            ] {
                let query = SchemaQuery {
                    for_command: None,
                    contract,
                };
                documents.insert(query.artifact_name(), Self::new(&query, document));
            }
            documents
        })
    }

    fn new(query: &SchemaQuery, mut document: Map<String, Value>) -> Self {
        Self::prune_definitions(&mut document);
        // Stay canonical even if a downstream crate enables serde_json/preserve_order.
        document.sort_keys();
        document.values_mut().for_each(Value::sort_all_objects);
        let digest = blake3::hash(&serde_json::to_vec(&document).expect("schema serializes"));
        let subject = query.for_command.map_or("session", Command::as_str);
        let id = format!(
            "urn:vvv:schema:{}:{subject}:{}:{digest}",
            crate::protocol::SCHEMA,
            query.contract.as_str()
        );
        document.insert("$id".into(), id.clone().into());
        document.sort_keys();
        Self { id, document }
    }

    fn generate<T: JsonSchema>(output: bool) -> Map<String, Value> {
        let mut schema = SchemaSettings::draft2020_12()
            .with(|s| {
                s.contract = if output {
                    schemars::generate::Contract::Serialize
                } else {
                    schemars::generate::Contract::Deserialize
                }
            })
            .into_generator()
            .into_root_schema_for::<T>()
            .to_value();
        Self::normalize(&mut schema);
        schema.as_object().expect("root schema object").clone()
    }

    // Flattened wire types can contribute the same required property twice.
    // JSON Schema requires unique names even though Serde permits this flattening.
    fn normalize(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if let Some(Value::Array(required)) = map.get_mut("required") {
                    let mut seen = BTreeSet::new();
                    required.retain(|name| {
                        seen.insert(name.as_str().expect("required property name").to_owned())
                    });
                }
                for child in map.values_mut() {
                    Self::normalize(child);
                }
            }
            Value::Array(values) => {
                for child in values {
                    Self::normalize(child);
                }
            }
            _ => {}
        }
    }

    fn request(
        root: &Map<String, Value>,
        command: Command,
        contract: SchemaContract,
    ) -> Map<String, Value> {
        let mut branch = root["oneOf"]
            .as_array()
            .expect("tagged request union")
            .iter()
            .find(|branch| branch["properties"]["command"]["const"] == command.as_str())
            .expect("every command has a request variant")
            .as_object()
            .expect("request object")
            .clone();
        branch.insert("$schema".into(), root["$schema"].clone());
        if let Some(defs) = root.get("$defs") {
            branch.insert("$defs".into(), defs.clone());
        }
        if contract == SchemaContract::Arguments {
            branch
                .get_mut("properties")
                .and_then(Value::as_object_mut)
                .expect("request properties")
                .remove("command");
            if let Some(required) = branch.get_mut("required").and_then(Value::as_array_mut) {
                required.retain(|v| v != "command");
            }
        }
        branch
    }

    fn result(command: Command, envelope: bool) -> Map<String, Value> {
        macro_rules! contract {
            ($ty:ty) => {
                if envelope {
                    Self::generate::<crate::protocol::Response<$ty>>(true)
                } else {
                    Self::generate::<$ty>(true)
                }
            };
        }
        match command {
            Command::DiscardPlan => contract!(crate::PlanReview),
            Command::ApplyPlan => contract!(crate::PlanReceipt),
            Command::ValidatePlan => contract!(crate::ValidationReport),
            Command::InspectPlan => contract!(crate::PlanReviewReply),
            Command::PrepareRename
            | Command::PrepareRewrite
            | Command::PrepareMove
            | Command::PrepareMoveSymbol => {
                contract!(crate::PlanReviewReply)
            }
            Command::ReviewPlan => contract!(crate::PlanReviewPage),
            Command::Schema => contract!(SchemaDocument),
            Command::SearchPage => contract!(crate::SearchPage),
            Command::ContextPage => contract!(crate::ContextPage),
            Command::Continue => contract!(crate::PageReply),
            Command::Expand => contract!(crate::Expansion),
            Command::Discover => contract!(crate::Discovery),
            Command::Context => contract!(crate::ContextReply),
            Command::Relationships => contract!(crate::Relationships),
            Command::Resolve => contract!(crate::ResolutionReply),
            Command::Navigate => contract!(crate::NavigationReply),
            Command::Search => contract!(crate::Search),
            Command::Outline => contract!(crate::Outline),
            Command::References => contract!(crate::References),
            Command::Where => contract!(crate::Locations),
            Command::Deps => contract!(crate::Deps),
            Command::Explain => contract!(crate::Explanation),
            Command::Surface => contract!(crate::Surface),
            Command::Impact => contract!(crate::Impact),
            Command::Dead => contract!(crate::Dead),
            Command::Imports => contract!(crate::ImportsReport),
            Command::WorkspaceFiles => contract!(crate::WorkspaceFiles),
            Command::File => contract!(crate::File),
            Command::Rewrite => contract!(crate::Rewrite),
            Command::Rename => contract!(crate::Rename),
            Command::Move => contract!(crate::Move),
            Command::MoveSymbol => contract!(crate::MoveSymbol),
            Command::SymbolMoveCandidates => contract!(crate::SymbolMoveCandidates),
            Command::Batch => contract!(crate::Batch),
            Command::History => contract!(crate::History),
            Command::Undo => contract!(crate::Undo),
        }
    }

    fn prune_definitions(root: &mut Map<String, Value>) {
        let Some(Value::Object(definitions)) = root.remove("$defs") else {
            return;
        };
        let mut needed = BTreeSet::new();
        Self::references(&Value::Object(root.clone()), &mut needed);
        let mut remaining: Vec<_> = needed.iter().cloned().collect();
        while let Some(name) = remaining.pop() {
            let mut nested = BTreeSet::new();
            Self::references(&definitions[&name], &mut nested);
            for name in nested {
                if needed.insert(name.clone()) {
                    remaining.push(name);
                }
            }
        }
        if !needed.is_empty() {
            root.insert(
                "$defs".into(),
                Value::Object(
                    definitions
                        .into_iter()
                        .filter(|(name, _)| needed.contains(name))
                        .collect(),
                ),
            );
        }
    }

    fn references(value: &Value, names: &mut BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                if let Some(name) = map
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|s| s.strip_prefix("#/$defs/"))
                {
                    names.insert(name.replace("~1", "/").replace("~0", "~"));
                }
                for value in map.values() {
                    Self::references(value, names);
                }
            }
            Value::Array(values) => {
                for value in values {
                    Self::references(value, names);
                }
            }
            _ => {}
        }
    }
}

impl crate::report::Document {
    pub(crate) fn schema(schema: &SchemaDocument) -> Self {
        use crate::protocol::display::{Line, Role};
        let mut document = Self::new();
        let json = serde_json::to_string_pretty(&schema.document).expect("schema serializes");
        document.body(json.lines().map(|line| Line::single(Role::Plain, line)));
        document
    }
}
