//! A parser-free engine and its observable storage; `$0` marks a source position.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vvv_engine::{Engine, Languages, MemoryVfs, Position, Retention, Vfs, Workspace};

use super::Fake;
use vvv_core::SourceText;

pub struct EngineFixture {
    pub vfs: Arc<MemoryVfs>,
    pub engine: Engine,
    cursors: BTreeMap<PathBuf, Position>,
}

impl EngineFixture {
    pub fn new(files: &[(&str, &str)]) -> Self {
        Self::with_language(files, Fake::default())
    }

    pub fn with_language(files: &[(&str, &str)], language: Fake) -> Self {
        let vfs = Arc::new(files.iter().fold(MemoryVfs::new(), |vfs, (path, text)| {
            vfs.with_file(Path::new("/ws").join(path), *text)
        }));
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(language),
        );
        Self {
            vfs,
            engine,
            cursors: BTreeMap::new(),
        }
    }

    pub fn marked(files: &[(&str, &str)]) -> Self {
        let sources: Vec<_> = files
            .iter()
            .map(|(path, text)| (*path, MarkedSource::new(text)))
            .collect();
        let files: Vec<_> = sources
            .iter()
            .map(|(path, source)| (*path, source.text.as_str()))
            .collect();
        let mut fixture = Self::new(&files);
        for (path, source) in sources {
            if let Some(cursor) = source.cursor {
                fixture.cursors.insert(path.into(), cursor);
            }
        }
        fixture
    }

    pub fn cursor(&self, path: &str) -> Position {
        self.cursors[Path::new(path)]
    }

    pub fn retaining(mut self, retention: Retention) -> Self {
        self.engine = self.engine.with_retention(retention);
        self
    }

    pub fn read(&self, path: &str) -> String {
        self.vfs.read(&Path::new("/ws").join(path)).unwrap()
    }

    /// The complete source tree, excluding engine-owned history.
    pub fn source_tree(&self) -> BTreeMap<PathBuf, String> {
        self.vfs
            .walk(Path::new("/ws"))
            .unwrap()
            .into_iter()
            .filter(|path| !path.starts_with("/ws/.vvv"))
            .map(|path| {
                let text = self.vfs.read(&path).unwrap();
                (path, text)
            })
            .collect()
    }
}

struct MarkedSource {
    text: SourceText,
    cursor: Option<Position>,
}

impl MarkedSource {
    fn new(marked: &str) -> Self {
        let offset = marked.find("$0");
        let text = SourceText::new(marked.replacen("$0", "", 1));
        assert!(!text.as_str().contains("$0"), "one cursor per fixture file");
        let cursor = offset.map(|offset| text.position(offset));
        Self { text, cursor }
    }
}
