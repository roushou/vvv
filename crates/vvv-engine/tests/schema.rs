#![cfg(feature = "schema")]
mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use vvv_engine::{
    Call, Command, Engine, Languages, MemoryVfs, SchemaContract, SchemaDocument, SchemaQuery,
    Workspace,
};

struct Contract {
    validator: jsonschema::Validator,
}

impl Contract {
    fn property_names(schema: &Value, root: &Value, names: &mut BTreeSet<String>) {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            names.extend(properties.keys().cloned());
        }
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            Self::property_names(
                root.pointer(reference.strip_prefix('#').unwrap()).unwrap(),
                root,
                names,
            );
        }
        for keyword in ["allOf", "oneOf", "anyOf"] {
            if let Some(branches) = schema.get(keyword).and_then(Value::as_array) {
                for branch in branches {
                    Self::property_names(branch, root, names);
                }
            }
        }
    }

    fn new(command: Option<Command>, contract: SchemaContract) -> Self {
        let schema = SchemaQuery {
            for_command: command,
            contract,
        }
        .execute()
        .unwrap();
        Self {
            validator: jsonschema::validator_for(&Value::Object(schema.document)).unwrap(),
        }
    }

    fn accepts(&self, value: &Value) {
        let errors: Vec<_> = self
            .validator
            .iter_errors(value)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{value}: {errors:?}");
    }

    fn rejects(&self, value: &Value) {
        assert!(
            !self.validator.is_valid(value),
            "unexpectedly accepted {value}"
        );
    }
}

#[test]
fn catalog_is_complete_and_all_documents_are_offline_valid_schemas() {
    assert_eq!(SchemaDocument::catalog().len(), Command::ALL.len() * 4 + 2);
    let mut ids = BTreeSet::new();
    let call = SchemaQuery {
        for_command: None,
        contract: SchemaContract::Call,
    }
    .execute()
    .unwrap();
    assert_eq!(
        call.document["oneOf"].as_array().unwrap().len(),
        Command::ALL.len()
    );
    for &command in Command::ALL {
        assert_eq!(serde_json::to_value(command).unwrap(), command.as_str());
        let arguments = SchemaQuery {
            for_command: Some(command),
            contract: SchemaContract::Arguments,
        }
        .execute()
        .unwrap();
        let value = Value::Object(arguments.document);
        let mut properties = BTreeSet::new();
        Contract::property_names(&value, &value, &mut properties);
        assert_eq!(
            properties,
            command
                .parameters()
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            "{command:?}"
        );
    }
    for schema in SchemaDocument::catalog().values() {
        let value = Value::Object(schema.document.clone());
        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(value["$id"], schema.id);
        assert!(ids.insert(&schema.id));
        jsonschema::draft202012::meta::validate(&value).unwrap();
        jsonschema::validator_for(&value).unwrap();
    }
}

#[test]
fn context_inputs_keep_defaults_strict_budgets_and_tagged_origins() {
    let schema = Contract::new(Some(Command::Context), SchemaContract::Arguments);
    let origin = json!({"kind":"position","path":"src/a.rs","position":{"line":0,"column":4}});
    schema.accepts(&json!({"origin":origin}));
    schema.accepts(&json!({"origin":origin,"budget":{},"future_field":true}));
    for bytes in [json!(1024), json!(16384), json!(1048576)] {
        schema.accepts(&json!({"origin":origin,"budget":{"max_bytes":bytes}}));
    }
    for bytes in [
        json!(1023),
        json!(1048577),
        json!("8192"),
        json!(1.5),
        Value::Null,
    ] {
        schema.rejects(&json!({"origin":origin,"budget":{"max_bytes":bytes}}));
    }
    schema.rejects(&json!({}));
    schema.rejects(&json!({"origin":{"kind":"guess","name":"Engine"}}));
    schema.rejects(&json!({"origin":origin,"budget":{"unknown":1}}));
    for selection in [
        json!("all"),
        json!({"ids":["abc"]}),
        json!({"ordinals":[1,2]}),
    ] {
        schema.accepts(&json!({"origin":origin,"selection":selection}));
    }
    schema.rejects(&json!({"origin":origin,"selection":{"ids":[3]}}));
    schema.rejects(&json!({"origin":origin,"selection":"first"}));
    let query: vvv_engine::ContextQuery = serde_json::from_value(json!({"origin":origin})).unwrap();
    assert_eq!(query.budget, vvv_engine::ContextBudget::default());
    // Runtime source validity is intentionally distinct from request shape.
    let vfs = Arc::new(MemoryVfs::new().with_file("/ws/a.p", "def Root"));
    let engine = Engine::new(
        Workspace::new("/ws", vfs),
        Languages::new().with(common::Fake::default()),
    );
    let request = json!({"command":"context","origin":{"kind":"position","path":"a.p","position":{"line":999,"column":0}}});
    Contract::new(Some(Command::Context), SchemaContract::Request).accepts(&request);
    let reply = serde_json::to_value(
        serde_json::from_value::<Call>(request)
            .unwrap()
            .execute(&engine),
    )
    .unwrap();
    assert_eq!(reply["status"], "error");
    Contract::new(Some(Command::Context), SchemaContract::Response).accepts(&reply);
}

#[test]
fn mutation_schema_enforces_lifecycle_and_preserves_nullable_input() {
    let output = schemars::generate::SchemaSettings::draft2020_12()
        .with(|s| s.contract = schemars::generate::Contract::Serialize)
        .into_generator()
        .into_root_schema_for::<vvv_engine::MutationState>()
        .to_value();
    let schema = Contract {
        validator: jsonschema::validator_for(&output).unwrap(),
    };
    schema.accepts(&json!({"applied":false}));
    schema.accepts(&json!({"applied":true,"history_id":7}));
    for bad in [
        json!({"applied":true}),
        json!({"applied":false,"history_id":7}),
        json!({"applied":false,"history_id":null}),
    ] {
        schema.rejects(&bad);
    }
    let input = schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<vvv_engine::MutationState>()
        .to_value();
    Contract {
        validator: jsonschema::validator_for(&input).unwrap(),
    }
    .accepts(&json!({"applied":false,"history_id":null}));

    let result = Contract::new(Some(Command::Batch), SchemaContract::Result);
    result.accepts(&json!({"intents":[],"applied":false,"files":[]}));
    result.accepts(&json!({"intents":[],"applied":true,"history_id":7,"files":[]}));
    result.rejects(&json!({"intents":[],"applied":true,"files":[]}));
    result.rejects(&json!({"intents":[],"applied":false,"history_id":7,"files":[]}));
}

#[test]
fn discovery_and_schema_queries_do_not_touch_the_workspace() {
    let vfs = Arc::new(common::FaultVfs::over(Arc::new(MemoryVfs::new())));
    let engine = Engine::new(Workspace::new("/ws", vfs.clone()), Languages::new());
    for request in [
        json!({"command":"discover"}),
        json!({"command":"schema","for_command":"context","contract":"arguments"}),
    ] {
        let call: Call = serde_json::from_value(request.clone()).unwrap();
        let command = call.request.command();
        Contract::new(Some(command), SchemaContract::Request).accepts(&request);
        let response = serde_json::to_value(call.execute(&engine)).unwrap();
        assert_eq!(response["status"], "ok");
        Contract::new(Some(command), SchemaContract::Response).accepts(&response);
    }
    assert!(vfs.trace().is_empty());
    let discovery = vvv_engine::DiscoveryQuery::default().execute(&engine);
    assert!(discovery.schemas_available);
    assert_eq!(discovery.commands.len(), Command::ALL.len());
    for capability in discovery.commands {
        let command: Command = capability.command.parse().unwrap();
        assert_eq!(command.is_read_only(), capability.read_only);
        let expected = SchemaQuery {
            for_command: Some(command),
            contract: SchemaContract::Arguments,
        }
        .execute()
        .unwrap();
        assert_eq!(capability.schemas.unwrap().arguments, expected.id);
    }
}

#[test]
fn session_schemas_preserve_envelopes_and_output_budget_constraints() {
    let call = Contract::new(None, SchemaContract::Call);
    call.accepts(
        &json!({"command":"search","name":"Root","id":{"client":1},"max_output_bytes":1024}),
    );
    call.accepts(&json!({"command":"search","name":"Root","max_output_bytes":null}));
    call.rejects(&json!({"command":"search","name":"Root","max_output_bytes":100}));
    call.rejects(&json!({"command":"unknown"}));
    let reply = Contract::new(None, SchemaContract::Reply);
    reply.accepts(&json!({"id":[1,2],"status":"ok","schema":1,"result":{"entries":[]}}));
    reply.accepts(
        &json!({"id":null,"status":"error","schema":1,"code":"bad_request","message":"invalid"}),
    );
    reply.rejects(&json!({"status":"error","schema":1}));
    for query in [
        SchemaQuery {
            for_command: None,
            contract: SchemaContract::Arguments,
        },
        SchemaQuery {
            for_command: Some(Command::Search),
            contract: SchemaContract::Call,
        },
    ] {
        assert!(matches!(
            query.execute(),
            Err(vvv_engine::EngineError::InvalidSchemaQuery)
        ));
    }
}

#[test]
fn source_wire_types_and_output_requirements_match_serde() {
    let schema = Contract::new(Some(Command::Search), SchemaContract::Result);
    schema.accepts(&json!({"query":{"name":"Root"},"matches":[]}));
    schema.rejects(&json!({"query":{"name":"Root"}}));
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(MemoryVfs::new().with_file("/ws/a.p", "def Root\nRoot")),
        ),
        Languages::new().with(common::Fake::default()),
    );
    let answer = engine
        .run(serde_json::from_value(json!({"command":"search","name":"Root"})).unwrap())
        .unwrap()
        .into_answer();
    let wire = serde_json::to_value(answer).unwrap();
    schema.accepts(&wire);
    assert!(wire["matches"][0]["content"].is_string());
    let mut legacy = wire["matches"][0].clone();
    legacy.as_object_mut().unwrap().remove("content");
    let match_input = schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<vvv_engine::Match>()
        .to_value();
    Contract {
        validator: jsonschema::validator_for(&match_input).unwrap(),
    }
    .accepts(&legacy);
    serde_json::from_value::<vvv_engine::Match>(legacy).unwrap();
    let path = schemars::schema_for!(vvv_engine::ModulePath).to_value();
    let schema = Contract {
        validator: jsonschema::validator_for(&path).unwrap(),
    };
    schema.accepts(&json!("crate::a::Root"));
    schema.rejects(&json!({"segments":["Root"]}));
}

#[test]
fn paged_contracts_validate_live_replies_and_reject_wrong_operation_kinds() {
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(MemoryVfs::new().with_file("/ws/a.p", "def Root\nRoot Root Root")),
        ),
        Languages::new().with(common::Fake::default()),
    );
    let search = json!({"command":"search_page","query":{"pattern":"Root"},"page":{"max_items":1}});
    Contract::new(Some(Command::SearchPage), SchemaContract::Request).accepts(&search);
    let reply = serde_json::to_value(
        serde_json::from_value::<Call>(search)
            .unwrap()
            .execute(&engine),
    )
    .unwrap();
    Contract::new(Some(Command::SearchPage), SchemaContract::Response).accepts(&reply);
    Contract::new(Some(Command::ContextPage), SchemaContract::Result).rejects(&reply["result"]);
    let next = json!({"command":"continue","cursor":reply["result"]["next_cursor"],"page":{"max_items":2,"max_bytes":2048}});
    Contract::new(Some(Command::Continue), SchemaContract::Request).accepts(&next);
    let reply = serde_json::to_value(
        serde_json::from_value::<Call>(next)
            .unwrap()
            .execute(&engine),
    )
    .unwrap();
    Contract::new(Some(Command::Continue), SchemaContract::Response).accepts(&reply);
    let context = json!({"command":"context_page","origin":{"kind":"position","path":"a.p","position":{"line":0,"column":4}},"work":{"max_lookups":1}});
    Contract::new(Some(Command::ContextPage), SchemaContract::Request).accepts(&context);
    let reply = serde_json::to_value(
        serde_json::from_value::<Call>(context)
            .unwrap()
            .execute(&engine),
    )
    .unwrap();
    Contract::new(Some(Command::ContextPage), SchemaContract::Response).accepts(&reply);
    Contract::new(Some(Command::SearchPage), SchemaContract::Result).rejects(&reply["result"]);
    let contract = Contract::new(Some(Command::SearchPage), SchemaContract::Arguments);
    for page in [
        json!({"max_items":0}),
        json!({"max_items":65}),
        json!({"max_bytes":1023}),
        json!({"max_bytes":1048577}),
        json!({"unknown":1}),
    ] {
        contract.rejects(&json!({"query":{"pattern":"Root"},"page":page}));
    }
    let expand = json!({"command":"expand","cursor":"bad","max_bytes":2048});
    let reply = serde_json::to_value(
        serde_json::from_value::<Call>(expand)
            .unwrap()
            .execute(&engine),
    )
    .unwrap();
    Contract::new(Some(Command::Expand), SchemaContract::Response).accepts(&reply);
    assert_eq!(reply["code"], "invalid_cursor");
    assert_eq!(reply["continuation"], "correct_cursor");
}
