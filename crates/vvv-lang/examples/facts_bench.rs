//! Reproducible full-extraction measurements over fixed source inputs.

use std::{hint::black_box, path::PathBuf, time::Instant};
use vvv_core::Language;

struct Corpus {
    root: PathBuf,
    files: Vec<PathBuf>,
}

impl Corpus {
    fn new() -> Self {
        Self {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
            files: Vec::new(),
        }
    }

    fn collect(&mut self, directory: PathBuf) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                self.collect(path);
            } else if path
                .extension()
                .is_some_and(|extension| matches!(extension.to_str(), Some("rs" | "ts" | "tsx")))
            {
                self.files.push(path);
            }
        }
    }

    fn inspect(&self, language: &dyn Language, path: &std::path::Path, output: &std::path::Path) {
        let source = std::fs::read_to_string(path).unwrap();
        let facts = language.facts(&source).unwrap();
        let relative = path.strip_prefix(&self.root).unwrap();
        let destination = output.join(relative).with_extension("facts");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, format!("{facts:?}")).unwrap();
    }

    fn snapshots(&mut self, output: &std::path::Path) {
        self.collect(self.root.join("crates/vvv/tests/corpus"));
        self.files.sort();
        for path in &self.files {
            match path.extension().and_then(|extension| extension.to_str()) {
                #[cfg(feature = "rust")]
                Some("rs") => self.inspect(&vvv_lang::rust::Rust::new(), path, output),
                #[cfg(feature = "typescript")]
                Some("ts") => self.inspect(&vvv_lang::typescript::TypeScript::new(), path, output),
                #[cfg(feature = "typescript")]
                Some("tsx") => self.inspect(&vvv_lang::typescript::Tsx::new(), path, output),
                _ => {}
            }
        }
    }

    fn measure(&self, language: &dyn Language, paths: &[&str]) {
        let mut sources: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read_to_string(self.root.join(path)).unwrap())
            .collect();
        if language.id() == vvv_core::LanguageId::new("typescript") {
            sources.push((0..100).map(|index| format!("function f{index}(input: number): number {{ const value = input; {{ const inner = value; service.run(inner); }} return value; }}\n")).collect());
        }
        for source in &sources {
            black_box(language.facts(source).unwrap());
        }
        let mut samples = Vec::new();
        for _ in 0..9 {
            let started = Instant::now();
            for source in &sources {
                black_box(language.facts(black_box(source)).unwrap());
            }
            samples.push(started.elapsed().as_micros());
        }
        samples.sort();
        println!(
            "{} bytes={} median_us={} min_us={} max_us={}",
            language.id(),
            sources.iter().map(String::len).sum::<usize>(),
            samples[4],
            samples[0],
            samples[8]
        );
    }
}

fn main() {
    let mut corpus = Corpus::new();
    if let Some(output) = std::env::args_os().nth(1) {
        corpus.snapshots(&PathBuf::from(output));
        return;
    }
    #[cfg(feature = "rust")]
    corpus.measure(
        &vvv_lang::rust::Rust::new(),
        &[
            "crates/vvv-engine/src/graph/navigation.rs",
            "crates/vvv-engine/src/graph/module_navigation.rs",
            "crates/vvv-core/src/paths/mod.rs",
        ],
    );
    #[cfg(feature = "typescript")]
    corpus.measure(
        &vvv_lang::typescript::TypeScript::new(),
        &[
            "crates/vvv/tests/corpus/ts-navigation/src/local.ts",
            "crates/vvv/tests/corpus/ts/src/app.ts",
            "crates/vvv/tests/corpus/ts-symbol-moves/src/move_selection.ts",
        ],
    );
}
