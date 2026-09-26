//! One fake language for every engine test, so no test links a parser.
//!
//! Its "syntax" is deliberately tiny:
//! - a pattern query matches the pattern as a whole word, together with an
//!   argument attached by a colon (`foo:1`), captured as `$NEXT`, or a
//!   bracketed sequence (`foo:[one, two]`) captured as `$$$NEXT`;
//! - a symbolic query (`--name`, `--symbol`) matches `def <name>` lines, each
//!   a `Function` declaration;
//! - references are whole words;
//! - imports are `use <path>` lines, slash-separated and workspace-relative;
//!   `use {path}` marks a grouped (non-rewritable) entry, `use <path>/*` a glob;
//! - the layout treats addresses as path components; the surgery renders them
//!   as paths and adds one side edit per move (a line in `manifest.p`) so
//!   side edits are exercised; an optional `package` manifest names the package.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use vvv_core::{
    Address, Capture, CaptureValue, Edit, Facts, ImportGroup, ImportRef, Language, LanguageId,
    Layout, Modifier, ModulePath, Package, Parsed, PathHead, PathSyntax, Project, Query, RawMatch,
    ReachKind, ResolveError, SearchError, Semantics, SideEdit, SourceText, Span, Surgery, Symbol,
    SymbolKind, VisibilityRule,
};

pub struct Fake {
    id: &'static str,
    extensions: &'static [&'static str],
}

impl Fake {
    pub fn new(id: &'static str, extensions: &'static [&'static str]) -> Self {
        Self { id, extensions }
    }

    /// The default fake: language `fake`, extension `.p`.
    pub fn default() -> Self {
        Self::new("fake", &["p"])
    }

    fn words(source: &str) -> Vec<(usize, &str)> {
        let mut out: Vec<(usize, &str)> = Vec::new();
        let mut start: Option<usize> = None;
        for (i, ch) in source.char_indices() {
            match (ch.is_alphanumeric() || ch == '_', start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    out.push((s, &source[s..i]));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            out.push((s, &source[s..]));
        }
        out
    }
}

/// Like Rust: paths join with `::`, importing a file scopes the file's name
/// and not its contents, only `def`s (functions) are addressable, and there
/// are no visibility modifiers.
const SEMANTICS: Semantics = Semantics {
    import_scopes_names: false,
    addressable: &[SymbolKind::Function],
    visibility: &[VisibilityRule::exact("pub", ReachKind::Everyone)],
    default_visibility: ReachKind::Declaring,
};

impl Language for Fake {
    fn id(&self) -> LanguageId {
        LanguageId::from(self.id)
    }

    fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    fn find(&self, source: &str, query: &Query) -> Result<Vec<RawMatch>, SearchError> {
        if query.is_symbolic() {
            return Ok(self
                .symbols(source)?
                .into_iter()
                .filter(|s| query.name().is_none_or(|n| n == s.name))
                .filter(|s| query.symbol().is_none_or(|k| k == s.kind))
                .map(|s| RawMatch {
                    symbol: Some(s.clone()),
                    ..RawMatch::plain(s.span, "def", &source[s.span.start..s.span.end])
                })
                .collect());
        }
        let needle = query.pattern_str().unwrap_or_default();
        Ok(Self::words(source)
            .into_iter()
            .filter(|(_, word)| *word == needle)
            .map(|(start, text)| {
                let end = start + text.len();
                let rest = &source[end..];
                // `foo:[one, two]` exposes a sequence capture whose expansion
                // must preserve the punctuation from the matched snapshot.
                if let Some(items) = rest.strip_prefix(":[")
                    && let Some(close) = items.find(']')
                {
                    let items_start = end + 2;
                    let captures = Self::words(&items[..close])
                        .into_iter()
                        .map(|(offset, word)| Capture {
                            span: Span::new(
                                items_start + offset,
                                items_start + offset + word.len(),
                            ),
                            text: word.to_owned(),
                        })
                        .collect();
                    let finish = items_start + close + 1;
                    return RawMatch {
                        captures: [("NEXT".to_owned(), CaptureValue::Multiple(captures))].into(),
                        ..RawMatch::plain(Span::new(start, finish), "word", &source[start..finish])
                    };
                }
                let next_len = match rest.strip_prefix(':') {
                    Some(arg) => arg.chars().take_while(|c| c.is_alphanumeric()).count(),
                    None => 0,
                };
                let next_start = if next_len > 0 { end + 1 } else { end };
                let next_span = Span::new(next_start, next_start + next_len);
                let next = Capture {
                    span: next_span,
                    text: source[next_span.start..next_span.end].to_owned(),
                };
                RawMatch {
                    captures: [("NEXT".to_owned(), CaptureValue::Single(next))].into(),
                    ..RawMatch::plain(
                        Span::new(start, next_span.end),
                        "word",
                        &source[start..next_span.end],
                    )
                }
            })
            .collect())
    }

    fn semantics(&self) -> &'static Semantics {
        &SEMANTICS
    }

    fn paths(&self) -> PathSyntax {
        PathSyntax::Posix
    }

    fn glob_marker(&self) -> Option<&'static str> {
        Some("/*")
    }

    fn facts(&self, source: &str) -> Result<Facts, SearchError> {
        let mut facts = Facts::new(self.symbols(source)?, self.imports(source)?, Vec::new());
        for (start, word) in Self::words(source) {
            facts.push_token(word, "word", Span::new(start, start + word.len()));
        }
        Ok(facts)
    }

    fn symbols(&self, source: &str) -> Result<Vec<Symbol>, SearchError> {
        Ok(source
            .match_indices("def ")
            .map(|(start, _)| {
                let name_start = start + 4;
                let name: String = source[name_start..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric())
                    .collect();
                let name_span = Span::new(name_start, name_start + name.len());
                let mut symbol = Symbol::plain(
                    SymbolKind::Function,
                    name,
                    name_span,
                    Span::new(start, name_span.end),
                );
                // `pub def x`: the modifier is part of the declaration.
                if let Some(at) = start
                    .checked_sub(4)
                    .filter(|at| &source[*at..start] == "pub ")
                {
                    symbol.visibility = Some(Modifier {
                        span: Span::new(at, at + 3),
                        text: "pub".to_owned(),
                    });
                    symbol.extent = Span::new(at, name_span.end);
                }
                symbol
            })
            .collect())
    }

    fn references(&self, source: &str, name: &str) -> Result<Vec<RawMatch>, SearchError> {
        Ok(Self::words(source)
            .into_iter()
            .filter(|(_, w)| *w == name)
            .map(|(i, w)| RawMatch::plain(Span::new(i, i + w.len()), "word", w))
            .collect())
    }

    /// `use <path>` lines declare (`as <name>` binds another name, `pub`
    /// re-exports); a `head::name` word anywhere else is a qualified
    /// reference, as a Rust `scoped_identifier` is.
    fn imports(&self, source: &str) -> Result<Vec<ImportRef>, SearchError> {
        let syntax = self.paths();
        let mut offset = 0;
        let mut found = Vec::new();
        for line in source.lines() {
            let start = offset;
            offset += line.len() + 1;
            let (line, reexport) = match line.strip_prefix("pub ") {
                Some(rest) => (rest, true),
                None => (line, false),
            };
            let start = start + if reexport { 4 } else { 0 };
            let Some(path) = line.strip_prefix("use ") else {
                let mut at = 0;
                for word in line.split(' ') {
                    if word.contains("::") {
                        found.push(ImportRef {
                            span: Span::new(start + at, start + at + word.len()),
                            path: PathSyntax::Scoped.parse(word),
                            group: None,
                            glob: false,
                            declares: false,
                            reexport: false,
                            alias: None,
                        });
                    }
                    at += word.len() + 1;
                }
                continue;
            };
            let span = Span::new(start + 4, start + line.len());
            found.push(
                match path.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
                    Some(inner) => ImportRef {
                        span: Span::new(span.start + 1, span.end - 1),
                        path: syntax.parse(inner),
                        group: Some(ImportGroup {
                            prefix: syntax.parse(""),
                            item: Span::new(span.start + 1, span.end - 1),
                            list: span,
                            items: 1,
                            statement: Span::new(start, start + line.len()),
                            top_level: true,
                        }),
                        glob: false,
                        declares: true,
                        reexport,
                        alias: None,
                    },
                    None => {
                        // `use a/x.p/foo as bar` binds `bar`; the span is the path.
                        let (path, alias) = match path.split_once(" as ") {
                            Some((path, alias)) => (path, Some(alias)),
                            None => (path, None),
                        };
                        let span = Span::new(span.start, span.start + path.len());
                        let import = match path.strip_suffix("/*") {
                            Some(dir) => ImportRef::new(
                                Span::new(span.start, span.end - 2),
                                syntax.parse(dir),
                            )
                            .glob(),
                            None => ImportRef::new(span, syntax.parse(path)),
                        };
                        let import = match alias {
                            Some(alias) => import.aliased(alias),
                            None => import,
                        };
                        if reexport {
                            import.reexporting()
                        } else {
                            import
                        }
                    }
                },
            );
        }
        Ok(found)
    }

    fn layout(&self) -> Option<&dyn Layout> {
        Some(&PathLayout)
    }

    fn surgery(&self) -> Option<&dyn Surgery> {
        Some(&PathSurgery)
    }
}

/// Addresses are path components under one package; imports are
/// workspace-relative paths.
pub struct PathLayout;

impl Layout for PathLayout {
    fn manifests(&self) -> &'static [&'static str] {
        &["package"]
    }

    fn package(&self, manifest: &Path, text: &str) -> Option<Package> {
        let name = text.trim();
        (!name.is_empty()).then(|| Package {
            id: name.into(),
            name: name.to_owned(),
            root: manifest.parent().unwrap_or(Path::new("")).to_path_buf(),
            dependencies: Vec::new(),
        })
    }

    fn address(&self, project: &Project, path: &Path) -> Result<Address, ResolveError> {
        Ok(Address::new(
            project
                .packages
                .containing(path)
                .map_or_else(|| "ws".into(), |package| package.id.clone()),
            path.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()),
        ))
    }

    fn candidates(&self, _: &Project, address: &Address) -> Vec<PathBuf> {
        vec![address.path().iter().map(|n| n.as_str()).collect()]
    }

    fn touched_by_move(
        &self,
        _: &Project,
        _: &Path,
        _: &Path,
    ) -> Result<Vec<PathBuf>, ResolveError> {
        Ok(vec![PathBuf::from("manifest.p")])
    }

    /// `ext/...` is an external package: unknowable, like a foreign crate.
    /// Paths are workspace-relative whatever syntax spelled them.
    fn resolve(&self, project: &Project, from: &Path, import: &ModulePath) -> Option<Address> {
        if import.head != PathHead::Named || import.first().is_some_and(|f| f.as_str() == "ext") {
            return None;
        }
        Some(Address::new(
            self.address(project, from).ok()?.package().clone(),
            import.segments.iter().cloned(),
        ))
    }
}

/// Renders addresses as paths and adds one side edit per move (a line in
/// `manifest.p`) so side edits are exercised.
pub struct PathSurgery;

impl Surgery for PathSurgery {
    fn render(&self, _: &Project, _: &Path, target: &Address, _: &ModulePath) -> ModulePath {
        ModulePath::new(
            PathSyntax::Posix,
            PathHead::Named,
            target.path().iter().cloned(),
        )
    }

    fn import_statement(&self, _: &Project, _: &Path, target: &Address, _: &str) -> Option<String> {
        Some(format!("use {}", target.display_with("/")))
    }

    /// `pub def x` — the only widening the fake spells.
    fn widen(&self, symbol: &Symbol, _: &SourceText, to: ReachKind) -> Option<Edit> {
        (to != ReachKind::Everyone).then(|| Edit::insert(symbol.span.start, "pub "))
    }

    fn relocate(
        &self,
        _: &Project,
        _: &Path,
        _: &Path,
        _: &[Parsed<'_>],
        _: Option<ReachKind>,
    ) -> Result<Vec<SideEdit>, ResolveError> {
        Ok(vec![SideEdit {
            path: PathBuf::from("manifest.p"),
            edit: Edit::insert(0, "moved\n"),
        }])
    }
}

/// The fake language, counting how often it is asked to parse a file.
pub struct Counting {
    inner: Fake,
    parses: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    layout: CountingLayout,
}

impl Counting {
    pub fn new(parses: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> Self {
        Self {
            inner: Fake::default(),
            parses,
            layout: CountingLayout {
                resolves: Default::default(),
            },
        }
    }

    /// Count the layout's resolutions too: one per import per fragment build.
    pub fn resolving(mut self, resolves: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> Self {
        self.layout.resolves = resolves;
        self
    }
}

/// `PathLayout`, counting how often it is asked to place a path.
pub struct CountingLayout {
    resolves: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Layout for CountingLayout {
    fn manifests(&self) -> &'static [&'static str] {
        PathLayout.manifests()
    }

    fn package(&self, root: &Path, manifest: &str) -> Option<vvv_core::Package> {
        PathLayout.package(root, manifest)
    }

    fn address(&self, project: &Project, path: &Path) -> Result<Address, ResolveError> {
        PathLayout.address(project, path)
    }

    fn candidates(&self, project: &Project, address: &Address) -> Vec<PathBuf> {
        PathLayout.candidates(project, address)
    }

    fn touched_by_move(
        &self,
        project: &Project,
        from: &Path,
        to: &Path,
    ) -> Result<Vec<PathBuf>, ResolveError> {
        PathLayout.touched_by_move(project, from, to)
    }

    fn resolve(&self, project: &Project, file: &Path, import: &ModulePath) -> Option<Address> {
        self.resolves
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        PathLayout.resolve(project, file, import)
    }
}

impl Language for Counting {
    fn id(&self) -> LanguageId {
        self.inner.id()
    }

    fn extensions(&self) -> &'static [&'static str] {
        self.inner.extensions()
    }

    fn semantics(&self) -> &'static Semantics {
        self.inner.semantics()
    }

    fn paths(&self) -> PathSyntax {
        self.inner.paths()
    }

    fn glob_marker(&self) -> Option<&'static str> {
        self.inner.glob_marker()
    }

    fn facts(&self, source: &str) -> Result<Facts, SearchError> {
        self.parses
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.facts(source)
    }

    fn find(&self, source: &str, query: &Query) -> Result<Vec<RawMatch>, SearchError> {
        self.inner.find(source, query)
    }

    fn symbols(&self, source: &str) -> Result<Vec<Symbol>, SearchError> {
        self.inner.symbols(source)
    }

    fn references(&self, source: &str, name: &str) -> Result<Vec<RawMatch>, SearchError> {
        self.inner.references(source, name)
    }

    fn imports(&self, source: &str) -> Result<Vec<ImportRef>, SearchError> {
        self.inner.imports(source)
    }

    fn layout(&self) -> Option<&dyn Layout> {
        Some(&self.layout)
    }

    fn surgery(&self) -> Option<&dyn Surgery> {
        self.inner.surgery()
    }
}

/// A scripted I/O boundary. Rules are armed after planning and match by path
/// and operation, so failures do not depend on timing or filesystem permissions.
pub struct FaultVfs {
    pub base: std::sync::Arc<dyn vvv_engine::Vfs>,
    rules: std::sync::Mutex<Vec<FaultRule>>,
    trace: std::sync::Mutex<Vec<(FaultOperation, PathBuf)>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultOperation {
    Read,
    Write,
    Rename,
    Inspect,
    PrepareParent,
    RemoveFile,
    RemoveDirectory,
}

#[derive(Debug, Clone)]
pub enum FaultAction {
    Before,
    Partial(String),
    After,
    Always,
    Occupy(String),
}

impl FaultAction {
    fn error(&self, path: &Path) -> vvv_engine::VfsError {
        vvv_engine::VfsError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::other(format!("injected {self:?} failure")),
        }
    }
}

struct FaultRule {
    operation: FaultOperation,
    path: PathBuf,
    skip: usize,
    action: FaultAction,
}

impl FaultVfs {
    pub fn over(base: std::sync::Arc<dyn vvv_engine::Vfs>) -> Self {
        Self {
            base,
            rules: Default::default(),
            trace: Default::default(),
        }
    }

    pub fn arm(&self, operation: FaultOperation, path: &Path, skip: usize, action: FaultAction) {
        self.rules.lock().unwrap().push(FaultRule {
            operation,
            path: path.to_path_buf(),
            skip,
            action,
        });
    }

    pub fn trace(&self) -> Vec<(FaultOperation, PathBuf)> {
        self.trace.lock().unwrap().clone()
    }

    pub fn clear_trace(&self) {
        self.trace.lock().unwrap().clear();
    }

    fn action(&self, operation: FaultOperation, path: &Path) -> Option<FaultAction> {
        self.trace
            .lock()
            .unwrap()
            .push((operation, path.to_path_buf()));
        let mut rules = self.rules.lock().unwrap();
        let index = rules
            .iter()
            .position(|rule| rule.operation == operation && rule.path == path)?;
        let rule = &mut rules[index];
        if rule.skip > 0 {
            rule.skip -= 1;
            return None;
        }
        if matches!(rule.action, FaultAction::Always) {
            return Some(rule.action.clone());
        }
        Some(rules.remove(index).action)
    }
}

impl vvv_engine::Vfs for FaultVfs {
    fn read(&self, path: &Path) -> Result<String, vvv_engine::VfsError> {
        if let Some(action) = self.action(FaultOperation::Read, path) {
            return Err(action.error(path));
        }
        self.base.read(path)
    }

    fn stamp(&self, path: &Path) -> Result<vvv_engine::Stamp, vvv_engine::VfsError> {
        self.base.stamp(path)
    }

    fn write(&self, path: &Path, contents: &str) -> Result<(), vvv_engine::VfsError> {
        match self.action(FaultOperation::Write, path) {
            Some(action @ (FaultAction::Before | FaultAction::Always)) => Err(action.error(path)),
            Some(action @ FaultAction::After) => {
                self.base.write(path, contents)?;
                Err(action.error(path))
            }
            Some(action @ FaultAction::Partial(_)) => {
                if let FaultAction::Partial(text) = &action {
                    self.base.write(path, text)?;
                }
                Err(action.error(path))
            }
            Some(action @ FaultAction::Occupy(_)) => Err(action.error(path)),
            None => self.base.write(path, contents),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.base.exists(path)
    }

    fn entry_kind(
        &self,
        path: &Path,
    ) -> Result<Option<vvv_engine::EntryKind>, vvv_engine::VfsError> {
        if let Some(action) = self.action(FaultOperation::Inspect, path) {
            return Err(action.error(path));
        }
        self.base.entry_kind(path)
    }

    fn prepare_parent(&self, path: &Path) -> vvv_engine::ParentCreation {
        match self.action(FaultOperation::PrepareParent, path) {
            Some(
                action @ (FaultAction::Before
                | FaultAction::Always
                | FaultAction::Partial(_)
                | FaultAction::Occupy(_)),
            ) => vvv_engine::ParentCreation::new(Vec::new(), Err(action.error(path))),
            Some(action @ FaultAction::After) => {
                let mut outcome = self.base.prepare_parent(path);
                outcome.result = Err(action.error(path));
                outcome
            }
            None => self.base.prepare_parent(path),
        }
    }

    fn remove_file(&self, path: &Path) -> Result<(), vvv_engine::VfsError> {
        match self.action(FaultOperation::RemoveFile, path) {
            Some(action @ FaultAction::After) => {
                self.base.remove_file(path)?;
                Err(action.error(path))
            }
            Some(action) => Err(action.error(path)),
            None => self.base.remove_file(path),
        }
    }

    fn remove_empty_dir(&self, path: &Path) -> Result<(), vvv_engine::VfsError> {
        match self.action(FaultOperation::RemoveDirectory, path) {
            Some(action @ FaultAction::After) => {
                self.base.remove_empty_dir(path)?;
                Err(action.error(path))
            }
            Some(action) => Err(action.error(path)),
            None => self.base.remove_empty_dir(path),
        }
    }

    fn move_if_absent(&self, from: &Path, to: &Path) -> Result<(), vvv_engine::MoveError> {
        use vvv_engine::{MoveError, MoveState};
        match self.action(FaultOperation::Rename, from) {
            Some(action @ (FaultAction::Before | FaultAction::Always)) => {
                Err(MoveError::new(action.error(from), MoveState::Unchanged))
            }
            Some(action @ FaultAction::After) => {
                self.base.move_if_absent(from, to)?;
                Err(MoveError::new(action.error(from), MoveState::Moved))
            }
            Some(action @ FaultAction::Partial(_)) => {
                if self
                    .base
                    .entry_kind(to)
                    .map_err(|error| MoveError::new(error, MoveState::Unchanged))?
                    .is_some()
                {
                    return Err(MoveError::new(
                        vvv_engine::VfsError::Exists(to.to_path_buf()),
                        MoveState::Unchanged,
                    ));
                }
                let contents = self
                    .base
                    .read(from)
                    .map_err(|error| MoveError::new(error, MoveState::Unchanged))?;
                self.base
                    .write(to, &contents)
                    .map_err(|error| MoveError::new(error, MoveState::Unknown))?;
                Err(MoveError::new(
                    action.error(from),
                    MoveState::DestinationLinked,
                ))
            }
            Some(FaultAction::Occupy(contents)) => {
                self.base
                    .write(to, &contents)
                    .map_err(|error| MoveError::new(error, MoveState::Unchanged))?;
                self.base.move_if_absent(from, to)
            }
            None => self.base.move_if_absent(from, to),
        }
    }

    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, vvv_engine::VfsError> {
        self.base.walk(root)
    }
}

pub struct FaultFixture {
    pub vfs: std::sync::Arc<FaultVfs>,
    pub engine: vvv_engine::Engine,
}

impl FaultFixture {
    pub fn new(files: &[(&str, &str)]) -> Self {
        let base = files
            .iter()
            .fold(vvv_engine::MemoryVfs::new(), |vfs, (path, text)| {
                vfs.with_file(Path::new("/ws").join(path), *text)
            });
        let vfs = std::sync::Arc::new(FaultVfs::over(std::sync::Arc::new(base)));
        let engine = vvv_engine::Engine::new(
            vvv_engine::Workspace::new("/ws", vfs.clone()),
            vvv_engine::Languages::new().with(Fake::default()),
        );
        Self { vfs, engine }
    }

    pub fn read(&self, path: &str) -> String {
        self.vfs.base.read(&Path::new("/ws").join(path)).unwrap()
    }

    pub fn arm(&self, operation: FaultOperation, path: &str, skip: usize, action: FaultAction) {
        self.vfs
            .arm(operation, &Path::new("/ws").join(path), skip, action);
    }
}
