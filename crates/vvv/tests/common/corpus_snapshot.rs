//! Lossless JSON expectations and shared, immutable source-preview expectations.

use std::collections::{BTreeMap, btree_map::Entry};
use std::fmt;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

/// Small records stay on one line; larger objects retain their readable structure.
/// Field values and array ordering are unchanged.
pub(super) struct JsonSnapshot<'a>(pub(super) &'a Value);

impl JsonSnapshot<'_> {
    fn write(&self, out: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
        let compact = serde_json::to_string(self.0).expect("JSON value serializes");
        if compact.len() <= 160 || (!self.0.is_object() && !self.0.is_array()) {
            return out.write_str(&compact);
        }
        let indent = (depth + 1) * 2;
        match self.0 {
            Value::Object(fields) => {
                out.write_str("{\n")?;
                for (index, (name, value)) in fields.iter().enumerate() {
                    write!(
                        out,
                        "{:indent$}{}: ",
                        "",
                        serde_json::to_string(name).expect("JSON key serializes")
                    )?;
                    Self(value).write(out, depth + 1)?;
                    if index + 1 < fields.len() {
                        out.write_str(",")?;
                    }
                    out.write_str("\n")?;
                }
                write!(out, "{:width$}}}", "", width = depth * 2)
            }
            Value::Array(items) => {
                out.write_str("[\n")?;
                for (index, value) in items.iter().enumerate() {
                    write!(out, "{:indent$}", "")?;
                    Self(value).write(out, depth + 1)?;
                    if index + 1 < items.len() {
                        out.write_str(",")?;
                    }
                    out.write_str("\n")?;
                }
                write!(out, "{:width$}]", "", width = depth * 2)
            }
            _ => unreachable!("scalars were written above"),
        }
    }
}

impl fmt::Display for JsonSnapshot<'_> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(out, 0)
    }
}

/// One fixture file's complete source preview, independent of the queried token.
pub(super) struct SourceSnapshot {
    name: String,
    value: Value,
}

impl SourceSnapshot {
    pub(super) fn new(corpus: &str, mut value: Value) -> Self {
        value.sort_all_objects();
        let path = value["path"].as_str().expect("source preview has a path");
        // Escape percent as well as separators, so flat and nested paths cannot
        // alias. Names remain readable and valid on every supported platform.
        let mut encoded = String::new();
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
                encoded.push(char::from(byte));
            } else {
                use std::fmt::Write;
                write!(&mut encoded, "%{byte:02X}").expect("writing to String succeeds");
            }
        }
        Self {
            name: format!("{corpus}__source__{encoded}__json"),
            value,
        }
    }

    pub(super) fn reference(&self) -> Value {
        static SOURCES: OnceLock<Mutex<SharedSources>> = OnceLock::new();
        let mut sources = SOURCES
            .get_or_init(|| Mutex::new(SharedSources::default()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sources.observe(self) {
            insta::assert_snapshot!(self.name.as_str(), JsonSnapshot(&self.value).to_string());
        }
        json!({"$snapshot": format!("{}.snap", self.name)})
    }
}

/// Every response must agree on a fixture file's complete preview. This check
/// also runs when snapshot updates are enabled, before any per-case replacement.
#[derive(Default)]
struct SharedSources {
    values: BTreeMap<String, Value>,
}

impl SharedSources {
    fn observe(&mut self, source: &SourceSnapshot) -> bool {
        match self.values.entry(source.name.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(source.value.clone());
                true
            }
            Entry::Occupied(entry) => {
                assert_eq!(
                    entry.get(),
                    &source.value,
                    "source preview differs between cases: {}",
                    source.name
                );
                false
            }
        }
    }
}

#[test]
fn json_layout_preserves_all_values_escaping_and_array_order() {
    let value = json!({
        "source": "é\n\t\"quoted\"\\".repeat(40),
        "records": [
            {"span": {"start": 4, "end": 9}, "kind": "identifier"},
            {"span": {"start": 0, "end": 3}, "kind": "keyword"}
        ],
        "values": [null, false, -3, 1.25, [3, 1, 2], {}]
    });
    let rendered = JsonSnapshot(&value).to_string();
    assert_eq!(serde_json::from_str::<Value>(&rendered).unwrap(), value);
    let nested = SourceSnapshot::new("fixture", json!({"path": "a/b.rs"}));
    for path in ["a__b.rs", "a%2Fb.rs"] {
        let flat = SourceSnapshot::new("fixture", json!({"path": path}));
        assert_ne!(
            nested.name, flat.name,
            "distinct paths need distinct snapshots"
        );
    }
}

#[test]
#[should_panic(expected = "source preview differs between cases")]
fn shared_previews_cannot_diverge_even_during_snapshot_acceptance() {
    let mut sources = SharedSources::default();
    let original = SourceSnapshot::new("fixture", json!({"path": "a.rs", "text": "original"}));
    assert!(sources.observe(&original));
    assert!(!sources.observe(&original));
    let changed = SourceSnapshot::new("fixture", json!({"path": "a.rs", "text": "changed"}));
    sources.observe(&changed);
}
