// Support fixtures.

fn model() -> Model {
    Model::new("~/dev/nx".into(), vec!["rust".into(), "typescript".into()])
}

fn preview(path: &str, lines: &[&str]) -> Event {
    let text = lines.join("\n");
    // Colour every `Language` token and the word `pub` like the real thing would.
    let mut highlights = Vec::new();
    for (i, _) in text.match_indices("Language") {
        highlights.push(vvv_engine::Highlight {
            span: vvv_engine::Span::new(i, i + 8),
            kind: vvv_engine::HighlightKind::Type,
        });
    }
    for (i, _) in text.match_indices("pub") {
        highlights.push(vvv_engine::Highlight {
            span: vvv_engine::Span::new(i, i + 3),
            kind: vvv_engine::HighlightKind::Keyword,
        });
    }
    highlights.sort_by_key(|h| h.span.start);
    Event::Previewed {
        identifiers: vec![],
        symbols: vec![],
        path: path.into(),
        text,
        highlights,
    }
}

/// Lines `1..=n`, each `// line k` except the given overrides.
fn numbered(n: usize, overrides: &[(usize, &str)]) -> Vec<String> {
    (1..=n)
        .map(|k| {
            overrides
                .iter()
                .find(|(line, _)| *line == k)
                .map_or_else(|| format!("// line {k}"), |(_, text)| (*text).to_owned())
        })
        .collect()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn typed(model: &mut Model, text: &str) -> Vec<Effect> {
    text.chars()
        .flat_map(|c| model.update(Action::Input(c)))
        .collect()
}

fn generation_of(effects: &[Effect]) -> u64 {
    match effects.last() {
        Some(Effect::Search { generation, .. } | Effect::Plan { generation, .. }) => *generation,
        other => panic!("expected a search or a plan, got {other:?}"),
    }
}

/// A model showing the search fixture's matches, as if the worker answered,
/// with the declaration's file previewed.
fn searched() -> Model {
    let mut m = model();
    let effects = typed(&mut m, "Language");
    let lines = numbered(
        70,
        &[
            (64, "pub trait Language: Send + Sync {"),
            (65, "    fn name(&self) -> &str;"),
            (66, "    fn extensions(&self) -> &[&str];"),
            (67, "}"),
        ],
    );
    let text = lines.join("\n");
    let start = text.find("pub trait").unwrap();
    let end = text.find("\n}").unwrap() + 2;
    let mut matches = fx::search().matches;
    let declaration = &mut matches[0];
    declaration.symbol.as_mut().unwrap().span = vvv_engine::Span::new(start, end);
    m.on_event(Event::Searched {
        generation: generation_of(&effects),
        matches,
        skipped: vec![],
    });
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    m.definition_preview(preview("src/lang/mod.rs", &refs));
    m
}

// Source fixtures stand in for the worker's already-resolved navigation reply.
impl Model {
    fn definition_reply(&mut self, event: Event) -> Event {
        let Event::Previewed {
            identifiers,
            path,
            text,
            highlights,
            mut symbols,
        } = event
        else {
            panic!("expected source fixture")
        };
        self.search.selection_changed();
        let declaration = self
            .search
            .results
            .current()
            .filter(|m| m.symbol.is_some() && m.path == path)
            .or_else(|| self.search.results.declarations().find(|m| m.path == path))
            .unwrap()
            .clone();
        let mut declaration = declaration;
        let symbol = declaration.symbol.as_mut().unwrap();
        symbol.extent = symbol.span;
        let symbol = declaration.symbol.as_ref().unwrap();
        if !symbols
            .iter()
            .any(|s| s.name_span == symbol.name_span && s.kind == symbol.kind)
        {
            symbols.push(symbol.clone());
        }
        let container = if symbol.kind == SymbolKind::Variant {
            symbols
                .iter()
                .find(|s| s.kind == SymbolKind::Enum)
                .unwrap_or(symbol)
        } else {
            symbol
        };
        let reference = |symbol: &vvv_engine::Symbol| vvv_engine::SymbolRef {
            language: declaration.language.clone(),
            declaration: vvv_engine::SourceAnchor {
                path: path.clone(),
                content: vvv_engine::ContentId::of(&text),
                span: symbol.extent,
            },
            name_span: symbol.name_span,
            kind: symbol.kind,
        };
        let target = reference(declaration.symbol.as_ref().unwrap());
        let container = reference(container);
        let (ticket, query) = self.search.body.pending().unwrap();
        Event::DefinitionResolved {
            ticket,
            query,
            reply: Ok(vvv_engine::NavigationReply {
                snapshot: vvv_engine::ContentId::of(&text).into(),
                outcome: vvv_engine::NavigationOutcome::Resolved {
                    target,
                    evidence: vvv_engine::ResolutionEvidence {
                        semantic: None,
                        addresses: vec![],
                    },
                    preview: Box::new(vvv_engine::DefinitionPreview {
                        selection: declaration.symbol.as_ref().unwrap().name_span,
                        container,
                        declaration,
                        source: vvv_engine::File {
                            identifiers,
                            path,
                            text,
                            highlights,
                            symbols,
                        },
                        identifiers: vec![],
                    }),
                },
            }),
        }
    }

    fn definition_preview(&mut self, event: Event) {
        let reply = self.definition_reply(event.clone());
        self.on_event(event);
        self.on_event(reply);
    }
}

/// `searched()`, then `r` on the declaration and the judge's answer.
fn renaming() -> Model {
    let mut m = searched();
    let effects = m.update(Action::Rename);
    m.on_event(Event::Planned {
        generation: generation_of(&effects),
        planned: rename_plan(),
    });
    m
}

/// The plan a `Language → Lang` rename answers with: the judged occurrences
/// and one real diff per file the default selection edits.
fn rename_plan() -> Planned {
    let rename = fx::rename(1);
    Planned::Rename {
        declarations: rename.declarations,
        occurrences: rename.occurrences,
        files: rename_files(),
    }
}

/// The rename's files: real diffs of the lines the `✓` sites sit on.
fn rename_files() -> Vec<vvv_engine::protocol::FileChange> {
    let sources: [(&str, &[ChangedLine]); 3] = [
        (
            "src/lang/mod.rs",
            &[(
                64,
                "pub trait Language: Send + Sync {",
                "pub trait Lang: Send + Sync {",
            )],
        ),
        (
            "src/lib.rs",
            &[(
                26,
                "pub use lang::{Language, LanguageId};",
                "pub use lang::{Lang, LanguageId};",
            )],
        ),
        (
            "src/other.rs",
            &[(
                13,
                "    vvv::Language::default()",
                "    vvv::Lang::default()",
            )],
        ),
    ];
    sources
        .iter()
        .map(|(path, changes)| {
            let path = *path;
            let edits = fx::rename(1)
                .occurrences
                .iter()
                .filter(|o| {
                    o.m.path == std::path::Path::new(path)
                        && o.confidence == vvv_engine::Confidence::Resolved
                })
                .map(|o| vvv_engine::Edit::replace(o.m.span, "Lang"))
                .collect();
            rewrite_file(path, changes, edits)
        })
        .collect()
}

/// `searched()`, then `m` on the row and a destination that plans.
fn moving() -> Model {
    let mut m = searched();
    let effects = m.update(Action::MoveFile);
    let generation = generation_of(&effects);
    let mv = fx::move_file();
    if let Mode::Move(mode) = &mut m.mode {
        mode.from = mv.intent.from.clone().into();
        mode.to = vvv_engine::RelPath::from(mv.intent.to.as_path())
            .as_str()
            .to_owned();
    }
    m.on_event(Event::Planned {
        generation,
        planned: Planned::Move {
            intent: Intent::Move(mv.intent),
            respellings: mv.respellings,
            notices: mv.notices,
            files: mv.files,
        },
    });
    m
}

/// `searched()`, then `w`, a template, and the plan's diff.
fn rewriting() -> Model {
    let mut m = searched();
    m.update(Action::Rewrite);
    let effects = typed(&mut m, "Lang");
    let generation = generation_of(&effects);
    m.on_event(Event::Planned {
        generation,
        planned: Planned::Rewrite {
            files: rewrite_files(),
        },
    });
    m
}

/// A line a rewrite changes: its 1-based number, and the text before and after.
type ChangedLine<'a> = (usize, &'a str, &'a str);

/// The plan a `Language → Lang` rewrite makes: one `FileChange` per file, with
/// its edits and a real diff of the lines the rewrite touches.
fn rewrite_files() -> Vec<vvv_engine::protocol::FileChange> {
    let sources: [(&str, &[ChangedLine]); 3] = [
        (
            "src/lang/mod.rs",
            &[(
                64,
                "pub trait Language: Send + Sync {",
                "pub trait Lang: Send + Sync {",
            )],
        ),
        (
            "src/lang/registry.rs",
            &[(
                4,
                "use super::{Language, LanguageId};",
                "use super::{Lang, LanguageId};",
            )],
        ),
        (
            "src/lib.rs",
            &[
                (
                    26,
                    "pub use lang::{Language, LanguageId};",
                    "pub use lang::{Lang, LanguageId};",
                ),
                (41, "    Language::new()", "    Lang::new()"),
            ],
        ),
    ];
    sources
        .iter()
        .map(|(path, changes)| {
            let path = *path;
            let edits = fx::search()
                .matches
                .iter()
                .filter(|m| m.path == std::path::Path::new(path))
                .map(|m| vvv_engine::Edit::replace(m.span, "Lang"))
                .collect();
            rewrite_file(path, changes, edits)
        })
        .collect()
}

/// One file's change: its edits, and a diff of `changes` (`line`, before,
/// after) in a file of `// line k` filler.
fn rewrite_file(
    path: &str,
    changes: &[ChangedLine],
    edits: Vec<vvv_engine::Edit>,
) -> vvv_engine::protocol::FileChange {
    let last = changes.iter().map(|(l, ..)| *l).max().unwrap_or(0) + 2;
    let text = |side: usize| -> String {
        let mut out = (1..=last)
            .map(|k| {
                changes.iter().find(|(l, ..)| *l == k).map_or_else(
                    || format!("// line {k}"),
                    |(_, before, after)| {
                        if side == 0 {
                            (*before).to_owned()
                        } else {
                            (*after).to_owned()
                        }
                    },
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        out.push('\n');
        out
    };
    let path = vvv_engine::RelPath::from(path);
    vvv_engine::protocol::FileChange {
        diff: vvv_engine::protocol::Diff::between(&path, &path, &text(0), &text(1)),
        path,
        moved_to: None,
        edits,
    }
}

fn history_entry(id: u64) -> vvv_engine::HistoryEntry {
    vvv_engine::HistoryEntry {
        id,
        at: vvv_engine::protocol::vocabulary::Ago::now(),
        intent: fx::rename_intent("Config", "Settings"),
        files: 0,
        paths: Vec::new(),
        moves: Vec::new(),
    }
}

struct FrameFixture<'a> {
    model: &'a Model,
}
impl<'a> FrameFixture<'a> {
    fn new(model: &'a Model) -> Self {
        Self { model }
    }
    fn render(&self) -> String {
        self.render_size(90, 20)
    }
    fn render_size(&self, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                App::new(
                    self.model,
                    Painter::plain(),
                    vvv_engine::protocol::vocabulary::Ago::now(),
                )
                .render(f.area(), f.buffer_mut())
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ------------------------------------------------------------------ search

fn report() -> vvv_engine::report::Document {
    use vvv_engine::protocol::display::{Line, Role};
    use vvv_engine::report::{Block, Document};
    let mut doc = Document::default();
    doc.block_body(Block::Title("rename Config → Settings".into()));
    doc.block_body(Block::Line(Line::single(Role::Path, "src/a.rs")));
    doc.block_note(Block::Summary(Line::single(Role::Plain, "✓ #3  2 files")));
    doc
}

// ------------------------------------------------------------------ snapshots

fn anchored() -> Model {
    let mut m = searched();
    m.update(Action::Enter); // query → results
    let effects = m.update(Action::Enter); // results → enter the scope
    let generation = match effects.last() {
        Some(Effect::Query { generation, .. }) => *generation,
        other => panic!("expected a read request, got {other:?}"),
    };
    let mut references = fx::references();
    references.declarations[0] = m.search.results.matches[0].clone();
    references.occurrences[0].m = references.declarations[0].clone();
    m.on_event(Event::Answered {
        generation,
        answer: Box::new(Answer::References(references)),
    });
    m
}

struct DefinitionFixture {
    reply: vvv_engine::NavigationReply,
}
impl DefinitionFixture {
    fn new(name: &str, path: &str, text: &str) -> Self {
        use vvv_engine::{
            ContentId, DefinitionPreview, NavigationOutcome, NavigationReply, ResolutionEvidence,
            SourceAnchor, Span, SymbolRef,
        };
        let mut declaration = fx::decl(path, 0, SymbolKind::Struct, name, text);
        let start = text.find(name).unwrap();
        let symbol = declaration.symbol.as_mut().unwrap();
        symbol.span = Span::new(0, text.len());
        symbol.extent = symbol.span;
        symbol.name_span = Span::new(start, start + name.len());
        declaration.content = Some(ContentId::of(text));
        let symbol = symbol.clone();
        let target = SymbolRef {
            language: declaration.language.clone(),
            kind: symbol.kind,
            name_span: symbol.name_span,
            declaration: SourceAnchor {
                path: path.into(),
                content: ContentId::of(text),
                span: symbol.extent,
            },
        };
        let identifiers = [name, "Beta"]
            .into_iter()
            .flat_map(|name| {
                text.match_indices(name)
                    .map(move |(start, _)| SourceAnchor {
                        path: path.into(),
                        content: ContentId::of(text),
                        span: Span::new(start, start + name.len()),
                    })
            })
            .collect::<Vec<_>>();
        Self {
            reply: NavigationReply {
                snapshot: ContentId::of(text).into(),
                outcome: NavigationOutcome::Resolved {
                    target: target.clone(),
                    evidence: ResolutionEvidence {
                        semantic: None,
                        addresses: vec![],
                    },
                    preview: Box::new(DefinitionPreview {
                        container: target,
                        declaration,
                        selection: symbol.name_span,
                        identifiers: identifiers.clone(),
                        source: vvv_engine::File {
                            path: path.into(),
                            text: text.into(),
                            symbols: vec![symbol],
                            highlights: vec![],
                            identifiers,
                        },
                    }),
                },
            },
        }
    }
    fn reply(&self) -> vvv_engine::NavigationReply {
        self.reply.clone()
    }
    fn browsing(&self) -> (Model, vvv_engine::NavigationReply) {
        let reply = self.reply();
        let vvv_engine::NavigationOutcome::Resolved { preview, .. } = &reply.outcome else {
            unreachable!()
        };
        let mut m = model();
        let effects = typed(&mut m, "Alpha");
        m.on_event(Event::Searched {
            generation: generation_of(&effects),
            matches: vec![preview.declaration.clone()],
            skipped: vec![],
        });
        let (ticket, query) = m.search.body.pending().unwrap();
        m.on_event(Event::DefinitionResolved {
            ticket,
            query,
            reply: Ok(reply.clone()),
        });
        m.search.preview = m.search.body.preview.clone();
        m.search.focus = SearchPanel::Body;
        (m, reply)
    }
}
impl Default for DefinitionFixture {
    fn default() -> Self {
        Self::new(
            "Alpha",
            "a.rs",
            "struct Alpha {\n    first: Beta,\n    second: Beta,\n}",
        )
    }
}
struct FollowFixture {
    effects: Vec<Effect>,
}
impl FollowFixture {
    fn reply(self, reply: Result<vvv_engine::NavigationReply, vvv_engine::Failure>) -> Event {
        let [Effect::Follow { ticket, query }] = self.effects.as_slice() else {
            panic!("expected follow: {:?}", self.effects)
        };
        Event::Followed {
            ticket: *ticket,
            query: query.clone(),
            reply,
        }
    }
}
impl Model {
    fn pick_identifier(&mut self, name: &str) -> Vec<Effect> {
        self.update(Action::Follow);
        for c in name.chars() {
            self.update(Action::Input(c));
        }
        self.update(Action::MenuChoose)
    }
}

impl Model {
    fn large_file_results(&mut self) {
        let matches = (0..20)
            .flat_map(|file| {
                (0..30).map(move |line| {
                    let path = format!("src/file{file:02}.rs");
                    let mut m = fx::m(&path, file * 100 + line, 0, "Thing", "Thing()");
                    m.id = vvv_engine::MatchId::derive(&m.path, m.span, &format!("{file}-{line}"));
                    m
                })
            })
            .collect();
        self.search.results.replace(matches);
        self.search.selection_changed();
        self.on_event(Event::Viewport {
            width: 120,
            height: 24,
        });
    }

    fn mouse(
        &mut self,
        kind: ratatui::crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
    ) -> Vec<Effect> {
        let event = ratatui::crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
        };
        self.search_frame()
            .pointer(event)
            .map_or_else(Vec::new, |pointer| self.on_event(Event::Pointer(pointer)))
    }
}

struct WorkspaceFixture {
    model: Model,
}
impl WorkspaceFixture {
    fn new() -> Self {
        let mut model = Model::new("~/dev/nx".into(), vec!["rust".into()]);
        model.update(Action::Workspace);
        model.on_event(Event::WorkspaceFiles {
            generation: model.generation,
            paths: vec!["src/a.rs".into(), "src/b.rs".into()],
        });
        let mut fixture = Self { model };
        fixture.preview("src/a.rs");
        fixture.model.on_event(Event::Viewport {
            width: 110,
            height: 25,
        });
        fixture
    }
    fn preview(&mut self, path: &str) {
        let mut text = String::new();
        let mut symbols = Vec::new();
        for i in 0..45 {
            let name = if i == 31 {
                "target_语言".into()
            } else {
                format!("item_{i:02}")
            };
            let start = text.len();
            text.push_str(&format!(
                "fn {name}() {{ {}needle_{i:02}(); }}\n",
                " ".repeat(60)
            ));
            symbols.push(vvv_engine::Symbol::plain(
                SymbolKind::Function,
                name.clone(),
                vvv_engine::Span::new(start + 3, start + 3 + name.len()),
                vvv_engine::Span::new(start, text.len() - 1),
            ));
        }
        self.model.on_event(Event::Previewed {
            path: path.into(),
            text,
            symbols,
            highlights: vec![],
            identifiers: vec![],
        });
    }
    fn browse(&self) -> &crate::modes::workspace::WorkspaceBrowse {
        self.model.search.workspace.as_ref().unwrap()
    }
}
