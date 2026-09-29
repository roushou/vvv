//! Export contracts or verify checked-in artifacts without modifying them.
use std::io::Write;
use std::path::PathBuf;

use vvv_engine::SchemaDocument;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/schemas/v1");
    let write = std::env::args().any(|arg| arg == "--write");
    if write {
        std::fs::create_dir_all(&directory)?;
    }
    for (name, schema) in SchemaDocument::catalog() {
        let mut bytes = serde_json::to_vec_pretty(&schema.document)?;
        bytes.push(b'\n');
        let path = directory.join(name);
        if write {
            std::fs::File::create(path)?.write_all(&bytes)?;
        } else if std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            != Some(serde_json::Value::Object(schema.document.clone()))
        {
            return Err(format!("schema artifact differs: {}", path.display()).into());
        }
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "json")
            && !SchemaDocument::catalog()
                .contains_key(&entry.file_name().to_string_lossy().into_owned())
        {
            return Err(format!("unexpected schema artifact: {}", entry.path().display()).into());
        }
    }
    Ok(())
}
