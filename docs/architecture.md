# Architecture

A library with thin interfaces around it. Dependencies point inward; nothing below
knows about anything above, and the interfaces know only the engine — with one
exception: the CLI's composition root names the plugins the engine is built with.

```
crates/vvv   vvv-tui     the interfaces: CLI, picker
      │        │             the CLI's composition root (`languages.rs`) also names plugins
    vvv-engine            Engine: one entry point, `run`; protocol/ is what --json prints
         │
     vvv-lang             syntax/ (ast-grep adapter) + rust/, typescript/ behind features
         │
     vvv-core             the plugin contract: nouns + traits (no parser, no I/O)
```

One membership test per crate: core — _does a language plugin need it
to be one?_; lang — _is it a grammar or the ast-grep adapter?_; engine — _does it do
something to a tree, or cross to a client as data?_; the interfaces — _is it a
rendering or a keystroke?_ A type lives in the lowest crate whose test it passes,
never lower because a consumer could not otherwise reach it.

Each crate's `//!` doc is the authority on what it does; this file explains how they
fit and the rules that keep them apart.

## Crates

### `vvv-core` — the plugin contract

Pure data and traits: what a language is given and what it hands back. No tree-sitter,
no file system, no lifecycle; `serde` and `thiserror` are its only dependencies. A
language's rule tables read alike: a `SymbolRule`, `ImportRule` or `HighlightRule` is
built by `new(..)` or a kind constructor and scoped by `under(kind)` (a direct
parent) or `within(kind)` (an ancestor).

| module      | holds                                                                                                                                                                                                                                                             |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `text`      | `Span` (byte range), `Position` (line/char column), `LineIndex`, `SourceText`                                                                                                                                                                                     |
| `paths`     | `Name` (one identifier), `ModulePath` (a path as spelled: a `PathHead` — package root, here, `n` up, `Self`, named, root — and segments) and `PathSyntax` (how a language spells one: `Scoped` `::` or `Posix` `/`; parses import text once, spells it back once) |
| `lang`      | `Language` trait, `LanguageId`, `LanguageRegistry`                                                                                                                                                                                                                |
| `oracle`    | `Oracle` trait (`refers(file, span) -> Option<Referent>`): a second opinion on a token from something that knows more than syntax; `Referent` (a declaration's file and name span)                                                                                |
| `symbol`    | `SymbolKind`, `Symbol` (name, node, extent, modifier), `SymbolRule` (the declarative plugin contract, with `leading` kinds and where the modifier is)                                                                                                             |
| `facts`     | `Facts`: everything about one file from one parse — symbols, imports, highlights, every identifier token interned                                                                                                                                                 |
| `semantics` | `Semantics`: what syntax means — path separator, import scoping, addressable kinds, visibility modifier → `ReachKind`                                                                                                                                             |
| `highlight` | `HighlightKind`, `Highlight`, `HighlightRule`: syntax colouring as data                                                                                                                                                                                           |
| `import`    | `ImportRef` (a `ModulePath` at a span, grouped or not, declaring or a reference), `ImportRule`/`ImportGrammar` (where a grammar keeps import paths, which `PathSyntax` parses them, what re-exports and aliases look like, the text every glob spells)            |
| `resolve`   | `Address` (a `PackageId` + path; nothing holds across packages), `Packages` (members and their dependencies, renames included); `Resolver` (a module system: `address`, `resolve`, `render`) and `MoveRules` (`companions`, `relocate`, `regroup`); `SideEdit`    |
| `query`     | `Query` + `QueryBuilder`: structural (`pattern`, `kind`) and symbolic (`symbol`, `name`) halves                                                                                                                                                                   |
| `search`    | `RawMatch` (a match within one text, before it is tied to a file), `Capture`, `Role` (declaration / import / use, set by the searcher), `SearchError`                                                                                                             |
| `edit`      | `Edit`, `ChangeSet` (sorted, overlap-checked edits + file moves, no write method)                                                                                                                                                                                 |

### `vvv-lang` — languages

One crate, two kinds of module:

- `syntax/` — the **only** code that sees `ast_grep_core::Node`. `AstGrepSearcher<L>`
  compiles a `Query` into ast-grep matchers, walks the tree with a language's
  `SymbolRule` table and `ImportGrammar`, and lowers everything to
  `RawMatch`/`Symbol`/`ImportRef` before returning.
- `syntax::AstGrepLanguage<L>` — the `Language` impl every grammar-backed language
  shares: an id, extensions, a `Grammar` and its `Semantics`, and optionally a `Layout`
  and a `Surgery`.
- `rust/`, `typescript/` — one per language, each behind a Cargo feature that enables
  exactly one grammar of `ast-grep-language`, mirrored file for file: `grammar.rs`
  (`GRAMMAR` and `SEMANTICS`, all data), `layout.rs` (`Layout`: how the project is
  arranged — addresses, path resolution, package manifests — as pure functions of a
  `Project`, the workspace as data), `surgery.rs` (`Surgery`: how an edit is spelled —
  `render`, `regroup`, `relocate` — text and facts in, edits out), and a type alias plus
  constructor in `mod.rs`. Nothing in the crate touches a file system; the engine builds
  the `Project` from its walk and parses the files a move touches. Rename needs only the
  `Layout`; move needs both.

Language modules must not import `ast_grep_core`; that is a review rule, not a compiler
one. With no features the crate is just the searcher and has no tests.

### `vvv-engine` — the façade

`Engine::new(workspace, languages)` — `Workspace::disk(root)` for the binary and the
registry from its composition root (`vvv-rs`'s `languages.rs`), a `MemoryVfs` and a fake
language for tests; `with_retention`, and `with_oracle(Arc<dyn Oracle>)` for a host that has a
build or a language server to ask — and one entry point, `Engine::run(command)`. An engine runs commands and
nothing else: it brings the graph up to date, builds a `Context` (the graph and the
workspace) and lets the command answer against it. A **`Command`** is a request as
data — one of the protocol's intents or queries — with `type Output` and
`fn run(self, &mut Context) -> Result<Output>`, implemented in the module that holds
its components (`impl Command for RenameIntent` in `rename/`, `for MoveIntent` in
`move_file/`, the per-file queries in `answers.rs`, the whole-tree ones in
`understanding.rs`). A mutation answers with `Planned<T>` —
the answer as a preview (`applied: false`, `files` filled) with the plan(s) kept
beside it; `Intent` itself is a command answering `Planned<Answer>`, so a batch step,
the picker or `serve` plans any intent without matching its variants. `Apply` writes,
records history and hands the same answer back marked applied — it needs the
`Mutation` capability, which `Answer` has for its mutating variants; `UndoLast` and
`HistoryQuery` are commands too. `FileQuery` gives an interface a file with its highlights; `root()` and
`language_ids()` are the two facts about the session. Nothing about a command lives
on the engine: a new capability is a new request type and one `impl`. Every command
starts by bringing the `Graph` up to date — one walk — and asking it questions. Each
`Candidate` (a file with its language) answers `find`, `references` and `imports` for
itself, and parses once however many questions it is asked: its `Facts` are computed
on first use and shared. Per-file work runs in parallel with `rayon` and collects in
path order.

The `Graph` (`graph/`) is what vvv knows about the tree and the questions commands ask
of it. The store: every walked file, the loaded text and facts of every claimed one,
and — on first use — the `Project` each language's layout resolves paths against (the
file set plus the packages its manifests declare). The questions, each answered
through a `Namespace` (one language's layout, surgery, semantics and project bound
together, so no command fetches those four itself): `search(query)` (files spelling
the query's literals, parsed and searched in parallel, declarations first with their
address), `declarations(query)` (by name, kind, language, `declared_in`),
`imports_of(path)` (each import resolved to an address, followed to its `origin`
and the file declaring that), `importers(ns, path, leads)` (every file of the
language whose declared imports lead where `leads` says — `deps` and `explain` are the
same query with different `leads`, both accepting any address `aliases_of` gives),
`references(query)` (the `Evidence`: the declarations called a name, the `Target`
among them, and every token spelling it judged through its file's `Scope` — the
reference edges, derived per name — then, for the tokens the scope left `?`, the
`Oracle` the engine was given, if any, whose answer counts only as a declaration the
graph knows: `Reason::Oracle` for the target, `OracleOther` for another), `aliases_of(ns, target, name)` (every address a
`pub use` chain makes reach the declaration, under whatever name each binds,
followed to a fixed point), `origin_of(ns, address)` (the
other direction: the declaration an address reaches through re-exports),
`consumers(ns, address, except)` (modules with an import or path under an address),
`fragments(ns)` (below), `namespace_of(path)`, `file(path)`, `files(language)`,
`containing(language, literals)`. `Retention` says how long it lives. `PerCall` (the default, the
CLI) reads the workspace afresh per command; nothing is stamped, because nothing will be
compared. `Session { trust }` (the TUI) keeps files and facts between commands,
stamps what it loads (`Vfs::stamp`: mtime and size on disk, a version counter in
memory) and on the next command re-reads only what changed; it still walks, so new and
deleted files are seen — except within `trust` of the last walk, when it does not look
at all: a burst of keystrokes walks a large tree once (the walk is the floor on a
76k-file tree, ~230 ms), and the engine's own writes (`Apply`, `UndoLast`) or
`Engine::touched()` (the TUI after an editor hand-off) end the trust at once.
One command uses one graph however many questions it asks — a
rename's declarations, its other declarations, its occurrences and its project all come
from the same walk.

A file's **`Fragment`** (`graph/fragment.rs`) is its structural edges held as data:
its module address, every addressable declaration placed (`Declared`: symbol, address,
reach) and every import statement and qualified path resolved (`Edge`: the `ImportRef`
and the address the layout gave it, or the address another import's binding leads to
when the path's head is a name the file imports). A candidate builds it once per
(file stamp, project build) and keeps it, so a session that asks ten whole-tree
questions resolves each file once; the project's build generation is what makes a
manifest change invalidate every fragment at once. A file's **`Scope`** — what it
sees: bound names, opened modules, resolved and unresolved paths — is read off the
fragment and kept beside it, so `references` judges tokens per name through a lookup;
only a token in the middle of a path costs a resolution of its prefix. `aliases_of`
reads only files spelling one of the names found so far, or — for glob re-exports —
the language's `glob_marker` (`::*` in Rust) in the alias's own package or naming it.

| command            | what it asks                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Query`            | `Graph::search`: `containing(literals)`, then each `Candidate::find(query)`, in parallel; declarations get their address from the namespace and move to the front                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `RewriteIntent`    | `search`, `Selection::narrow`, one `Edit` per match from the `Template`, `Plan::new`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `RenameIntent`     | declarations by name (narrowed by `declared_in`); per declaring language: a `Target` (module address + name, via the language's `Resolver`) when one path-addressable declaration is meant — two refuse and ask for `declared_in` — then `Graph::containing(name)` and `Target::judge` on each file (each token judged by a `Scope` built from the file's imports; a token ending a path is judged by the path, or `?` when its head is unknown); default selection by confidence; `Selection::narrow`, one `Edit` per occurrence, `Plan::new`                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `MoveIntent`       | validate paths and language, `Graph::namespace_of` (layout, surgery, project in one), compute the `MoveSet` (a file, or every file under a directory, plus the layout's companions such as Rust's `a.rs` ↔ `a/`), then `Rebase::rewrite` on every `Graph::files(language)` (re-render every import resolving under the old address, moved files' own imports from their new locations — each a `Change`), `Reachability::check` on the references it touched, a `Widen` per violation (an edit, or a notice across a package boundary), the surgery's side edits (`relocate`, told what the moved `mod` line needs), record every move, `Planned::of`                                                                                                                                                                                                                                                                                                                                   |
| `MoveSymbolIntent` | the graph establishes the situation — the `Extraction` (the declaration and its pieces; `impl` blocks are `Impl` symbols named after their type), both files parsed, `Graph::consumers` of the old address, `Graph::references` for the old file's remaining uses — then `SymbolMove` runs its operations over it, each appending to one `Change`: qualified paths inside the moved text re-rendered from the new file; the old file's imports and siblings the text names become imports in the new file (siblings `Widen`ed if needed); `Rebase::of_addresses` rewrites every consumer; a bare use left in the old file imports it back; an import of it in the new file is deleted; the declaration is `Widen`ed for consumers its reach at the new module no longer admits; last, the cut (`Extraction::cuts`) and the paste (`Extraction::assemble`, which carries the edits that fell inside the moved text) at `Surgery::item_insertion`, imports at `Surgery::import_insertion` |
| `BatchIntent`      | `Workspace::staged()` (an `Overlay` the real files never see); each intent planned by an engine over it and applied to it, receipts chained with `Receipt::then`; the preview is every touched file now against the staging tree at its final path                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `Request`          | the command it names, run against the same context, its result as the `Answer` of that name; a mutation with `apply` runs the intent then `Apply` on what it planned (`request.rs`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `Apply(planned)`   | each plan's `Plan::apply` in order (one for a command, the steps for a batch); a failure rolls the earlier receipts back; one `History::push` of `T::intent()`; the result comes back with `applied` and `history_id` set                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `UndoLast`         | `History::last` → `Receipt::undo` (fingerprint check, then rollback) → `History::pop`; answers `Undo` with what was restored                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `HistoryQuery`     | `History::entries`, each record's entry                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `SurfaceQuery`     | every fragment's declarations in the package; each public one, or one `aliases_of` offers elsewhere, listed with its aliases and how many other fragments import any of its addresses                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `ImpactQuery`      | `references` for the declaration, `aliases_of` for its addresses, then breadth first over fragments: a module whose imports lead under a frontier address joins the next ring, once, at the depth it is first reached                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `DeadQuery`        | for each placed declaration (a type and its impls once), `references(name declared_in file)`: no `Resolved` token beyond its own name spans means unreferenced, `Unresolved` tokens are counted as `unsure`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `ImportsQuery`     | each fragment's declared imports: no address is unresolved; the same (address, glob) twice is redundant; a binding (`ImportRef::binding`, alias or last segment) no token outside the statement spells is unused, skipped for re-exports and for languages whose imports hide which names they take; files with no module address are `unplaced`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `FileQuery`        | the file loaded, `Language::highlights` from the language claiming it                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |

The engine's supporting modules:

| module                                                                                          | holds                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `vfs`                                                                                           | `Vfs` trait (read, write, walk, `stamp`); `MemoryVfs` for tests, `DiskVfs` (`.gitignore`-aware parallel walk), `Overlay` (writes over a base that is never touched — how plans compose)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `workspace`                                                                                     | `Workspace` = root + `Arc<dyn Vfs>`; `SourceFile`; relative/absolute path handling                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `change`                                                                                        | `Change`: what an operation proposes before it is bound to the tree — edits by file, files moved, notices, respellings. Every step of a mutation (a `Rebase` rewrite, a widening, a relocation) answers with one; the command merges them and `Engine::planned` binds the result to a `ChangeSet` (overlaps refused there, once), previews it and keeps notices and respellings for the answer                                                                                                                                                                                                                                                                                                                           |
| `plan`                                                                                          | `Plan` (change set + fingerprints) → `preview` / `apply` → `Receipt` (with post-apply fingerprints) → `rollback` / `undo`; `Planned<T>`, a result with its plans and preview, `Deref` to the result                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `protocol`                                                                                      | everything `--json` prints and nothing that touches a tree: intents, `Match`/`MatchId`/`Occurrence` with `Confidence` and `Reason`, `Selection`, the answers (`Outline`, `Deps`, `Explanation`, `References`, `Locations`, `File`), `Notice`, `Respelling`, `Reach`, `Template`, one result per command, `HistoryEntry`, `FileChange` with a `Diff` (its `Hunk`s read from `similar`, rendered to the wire string), `SCHEMA` and the `Response` envelope, `Request`/`Answer` (every command as one value and every result as one), `Call`/`Reply` (a request with an `id` and its reply, what `vvv serve` speaks), `Failure` with its `ErrorCode` (what an error is on the wire; `EngineError::code()` and `::hint()` say which), `display` — the shared styled-line |
| IR (`Role`/`Piece`/`Line`, `hit`, `diff`, `counts`) each interface renders in its own colours — |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| and `vocabulary` — how the answers are read (below). Documented in [protocol.md](protocol.md)   |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |

`history.rs` is the undo stack: `.vvv/history.json`, newest last, capped at 20 because a
receipt carries full pre-apply file contents. Each record stores the `Intent` that was
applied — data, never a sentence — and its receipt; the receipt never leaves the engine,
a client sees the `HistoryEntry` (id, time, intent, what was written). It goes through the
`Vfs` like everything else, so engine tests exercise it in memory.

`protocol::vocabulary` sits with the shapes so a
client reads them the way the CLI prints them: `Mark`, every glyph any interface
draws, its word (`? unverified`, `→ paths rewritten`), its one-line meaning, and
`From<Confidence>` / `From<Reason>`; `IntentLine`, the one place a request is put into
words (preview titles and `history` lines both use it); `Plural`; `Ago`. Colour is the
renderer's: the CLI's `Palette::mark`, the picker's `Theme::mark`.

Engine tests live in `tests/`, all against one shared fake language in
`tests/common/mod.rs` whose tiny syntax (whole-word patterns, `def name` declarations,
`use path` imports, a path-component layout) covers every command without a parser.

The engine names no language: `crates/vvv`'s `languages.rs` is the single
`#[cfg(feature)]`-gated spot outside `vvv-lang`, and features cascade
`vvv-rs → vvv-lang → ast-grep-language`. With none enabled `vvv-lang` is not even a
dependency of the binary, the engine still builds and passes every test against fake
languages, and it is then the light crate a JSON client links for the protocol types
alone.

`lib.rs` re-exports every core noun that appears in a protocol type (`Span`, `Symbol`,
`Address`, `LanguageId`, …) next to the protocol itself, so an interface writes
`use vvv_engine::…` and only that.

`report/` is the result as a document. `Document` — `Block` is `Title`, `Heading`,
`Section`, `Line`, `Summary`, `Note`, `Diff` or `Blank` — is built from an `Answer`
by `Document::of`, the per-command constructors living on `Document` itself, with the
row builders in `report/lines.rs` (`Declaration`, `Sections`, `Verdicts`,
`OutlineTree`, `DepGroups`, `ImporterRows`, `Respellings`, `NoticeRow`,
`HistoryLine`, `SiteLine`, `ImportSiteLine`, `PlacedLine`, `SkippedLine`, `Caret`,
`Diff`, `Tag`, `Verdict`) turning protocol data into `Line`s. It knows the business
and names no interface; both the CLI and the picker read it. `--json` is not this —
it is the `Answer`, the wire contract.

### `crates/vvv` — the entrypoint

Package `vvv-rs` (the bare name is taken on crates.io), binary `vvv`.

`Cli` (global `-C`, `--json`, `--color`) → `Context { engine, format }` (`Engine::new`)
→ `cli::commands::*Cmd` (`clap::Args`; fields are the command's own inputs;
`run(self, &Context)`) → a `Reporter` (`Human` or `Json`). A command builds a
`Request`; `Context::run` runs it and hands the `Answer` to the reporter: three lines,
no logic. Applying is the `Request`'s job (`apply: true`), the same path `serve` uses
— the per-command `--apply` dance is not repeated. `vvv serve`
is the exception that has none of its own: it reads a `Call` per line of stdin, runs
its `Request` on a session engine and writes the `Reply` — the transport an MCP or
editor adapter wraps. Dependencies: `vvv-engine` and, behind the `tui` feature (on by
default), `vvv-tui`; `vvv ui` or bare `vvv` in a terminal hands the engine to the
picker with the CLI's colour policy and `$VISUAL`/`$EDITOR`.

`output/` is the CLI's display layer. `mod.rs` is the strategy — `Reporter`
(`report(&Answer)`, `error(&anyhow::Error)`) and `OutputFormat`, the choice of
how results look. Two renderers implement it, and they share only `Diagnose`:

- `human/mod.rs` — `HumanReporter<O: Write, E: Write>`, the human renderer: a
  pipeline `Answer → Document → View → Presentation → Styled → text`. It builds
  the report (`vvv_engine::report::Document::of`) and draws it
  (`Renderer::render`), writing results to `O` (stdout), notes to `E` (stderr).
- `json.rs` — `JsonReporter<W: Write>`, one `Response` envelope per invocation.
  It serializes the `Answer` itself: no document, no view, no colour.
- `render/` — the drawing side, which names no command:
  - `mod.rs` — the `Renderer` trait: an interface takes a [`Document`] and draws it.
  - The layout itself — a `Document` as `Line`s — lives in the engine, in
    `vvv_engine::report::view`: the `View` trait, its `Presentation`, and the
    `Detailed` view. The CLI picks the view and styles what it returns.
  - `palette.rs` — `Palette`, the colour policy (`--color`, `NO_COLOR`,
    `TERM=dumb`, TTY detection, shared with the picker through
    `Palette::enabled`; `Palette::plain()` renders no escape codes, which is
    what tests use; `Palette::role` maps a `display::Role`), and `Painted`.
  - `style.rs` — `Styled`, a `display::Line` in the palette's colours.
- `diagnosis.rs` — `Diagnose`: an `anyhow::Error` as the wire's `Failure`, the
  engine's own code and hint when it is the engine's, `bad_query`/`bad_selection` for
  what only the CLI can get wrong. Both reporters print errors through it.
- `fixtures.rs` + `human/snapshots/` — hand-built protocol values and `insta`
  snapshots of the exact plain-text output of every view. Changing a word changes a
  snapshot; review the diff, then `INSTA_UPDATE=always cargo test -p vvv-rs` to
  accept.

[`Document`]: ../vvv_engine/report/struct.Document.html

`tests/corpus.rs` is the gate on meaning: two small workspaces under
`tests/corpus/` — a Rust workspace of two crates with child modules, re-export chains,
a glob re-export, an alias, an inline test module; a TypeScript project with relative
imports and `export … from` — and every command run over them through the binary,
JSON and human, each output an `insta` snapshot under `tests/corpus/snapshots/`. The
same file loads each corpus into a `MemoryVfs` and checks, for every mutation, that
what is applied is what was previewed (each previewed file with its edits made, at
the path the preview said, and nothing else touched), that an apply undone leaves
every file as it was, and that a batch of two applied equals the second applied after
the first, one undo reverting both.

### `vvv-tui` — the picker

A four-method surface: `Tui::new(engine)` (a `Retention::Session` engine trusting its
walk for a second),
`.editor(Option<String>)` (what `e` runs, `+line path` appended; the caller resolves
`$VISUAL`/`$EDITOR`), `.color(bool)` (the caller decides), `.run() -> Result<(), Error>`.
Everything else is private. Depends on `vvv-engine` (the binary picks the languages) and
`ratatui`. Modes of panels, Elm-shaped
so it is testable without a terminal. The applied result is shown too: the worker
builds the `Document` (`vvv_engine::report`) from the applied `Answer`, and
`Overlay::Report` draws it and walks its source rows (`j`/`k`, `e`).

- `error.rs` — the crate's public `Error`: terminal I/O, the engine, the editor.
- `model.rs` — all state as data: the `Search` hub (query, results, context), the
  current `Mode` (`Rename`, `Move`, `Rewrite`, `History`, each with its input, its
  rows, its panel cursors and a focus enum implementing `Panels`), an optional
  `Overlay` (menu, confirm, help), status.
- `action.rs` — `Action` (what the user did, generic across modes: `Input`, `Enter`,
  `Toggle`, `FocusNth`…), `Effect` (what to ask the engine: `Search`, `Plan`,
  `Commit`, `Preview`, `History`, `Undo`; `Edit` for the loop itself), `Event` (what
  came back; `Planned` carries what a mode shows about its intent). Plain enums.
- `keymap/` — the key vocabulary, pure. `keys.rs`: `Key` (a `Code` and
  `Modifiers`) and its constructors. `mod.rs`: `Trigger` is what a
  `Keybinding` listens for (`Key`, `Text`, `Any`), `Dispatch` is what it does
  (`Run(A)`, `Type`), `Legend`/`Bar` say how it reads, and `Layer` is a named
  set of bindings with `resolve` and `rows`. `input.rs`: the one place a
  crossterm event becomes a `Key`, shift folded into the character.
- `screen/` — a `Screen` is the keys that work from any of its panels (`layer`),
  the `Panel`s it is made of, and a `layout` function. A `Panel` is one region:
  its own `layer`, the `PanelKind` that selects the shared defaults, and a
  `content` function. `screen/defaults.rs` holds the shared `Layer`s — `GLOBAL`,
  `NAVIGATE`, `DIGITS`, `LIST`, `TEXT` — and `default_for(PanelKind)`. Each mode
  file owns its layers and its panels: `search.rs` defines `SEARCH`, `rename.rs`
  `RENAME`, and so on; `overlay.rs` defines the three overlay screens.
- `render/` — the drawing primitives the screens compose. `Painter` owns the
  palette `Theme` and answers the drawing questions with one receiver (`caret`,
  `site`, `hit`, `line`, `source_window`); the colour policy is reachable through
  the painter (`Theme::role` maps a `display::Role`, `Theme::mark` colours a
  `Mark`). The stateful boxes are `Pane` (a bordered list/text box) and `Header`
  (the title card); a screen's `layout` splits a `Region` into panel regions
  (`columns`, `split`, `rows`), and `Fit` is a text helper. Words come from
  `protocol::vocabulary`.
- `model.rs` — `Model::mode_screen()`/`overlay_screen()` pick the active
  `Screen`, `Model::focus()` the focused panel's index, and `Overlay::Help`
  keeps the screen and focus it was opened over so the key list stays about them.
- `update.rs` — `Model::action_for(event)` resolves through
  `Model::screen().resolve(focus, key, holds)`: the globals first, then the
  focused panel's layer, the screen's, the panel kind's defaults, then navigation.
  `Model::update` and `Model::on_event` are pure and return effects. Searches and plans carry a
  generation so stale answers are dropped; a mode's input (name, destination,
  template) drives a plan the way the query bar drives a search. Entering a mode
  sends its first plan at once (no debounce) and sets `arriving`: keys already go
  to the mode, but `Model::shown` keeps the screen on search until the plan answers,
  so the new layout appears filled rather than empty and then filled.
- `worker.rs` — the engine on its own thread; effects in, events out; bursts of
  searches or plans are coalesced. `Commit` plans and applies in one step.
- `tui.rs` — `Tui` and the only I/O: terminal setup, the event loop with
  debounced searches and plans, the editor hand-off, teardown.
- `fixtures.rs` + `tests.rs` — `update` with plain assertions; every mode rendered into
  ratatui's `TestBackend` and snapshotted with `insta` (`INSTA_UPDATE=always cargo test
  -p vvv-tui` to accept).

Both interfaces render the shared `protocol::display` styled-line IR: the CLI maps
a `Role` through its `Palette` to ANSI, the picker maps it through its `Painter`'s
`Theme` to ratatui. The composition of a match's `hit` line, a file's `diff` lines,
and the `✓ 3  ? 1  ✗ 0` counts line is defined once, in the protocol.

Selection in the TUI _is_ the CLI's `--select`: the rename and rewrite modes' ticked
rows are content-derived ids fed to `Selection::Ids`.

## Boundaries

**Plugin boundary is data-only.** `Language::{find, symbols, references}` take `&str`
and return owned, serializable values. Consequences: the core and the engine are
testable with a fake language; a language could move behind a process or WASM
boundary without touching planners; `vvv-core` compiles in seconds because no C grammar
is in its graph.

**Declarative grammar knowledge.** A plugin says _"`function_item` with field `name`
declares a `Method` when inside `impl_item`"_ as a `SymbolRule`; the traversal that
applies it lives once, in `vvv-lang/src/syntax/`. Adding a language is adding a table.

**Writes happen in exactly one place.** `Plan::apply`. It stages every file first
(read, fingerprint check, compute), then writes — renaming first when a file moves —
and restores on failure. `Receipt::rollback` undoes moves in reverse before restoring
contents. `ChangeSet` has no `apply` method.

**Errors and notices are variants, not sentences.** `ResolveError` has one variant per
situation a layout can refuse (`Root`, `HasChildren`, `CrossProject`, `NoParentFile`
with the candidate files, …) and `Notice` carries a `NoticeKind`. `thiserror` gives
errors a canonical message; the CLI layers hints on top by matching variants, and JSON
clients get the fields, not the prose.

**Rename resolves through imports, not types.** `rename/scope.rs` builds, per file,
what its imports bring in: names (`use a::b::X`, grouped entries), opened modules
(globs, or any file import where the grammar hides the names), and qualified paths.
A token is `Resolved` if the file is the declaring module, imports the name, opens
the module, or spells a path resolving to the target; `Other` if any of those point
at a different same-named declaration; `Unresolved` otherwise. Methods, fields and
variants (`SymbolKind::is_path_item() == false`) get no target and stay syntactic.
Ambiguity is per language: a Rust and a TypeScript `foo` never compete.

**Visibility is computed, never guessed**. After
`Rebase` has rewritten a move's paths it reports every reference it touched as (the
module it is read from afterwards, what it names afterwards). `move_file/reachability.rs`
walks each target's address from the package root down: every `mod` on the way and the
item itself is looked up in the facts of its declaring file — as the tree is now, so a
moved address is rebased back to find it — and its `Reach` (modifier × declaring module,
via `Semantics`) must admit the consumer. A declaration some consumer cannot see gets the
narrowest reach real code writes that covers all its consumers: `pub(super)` when the
lowest common ancestor is its parent, `pub(crate)` otherwise; `Surgery::widen` spells
it. A consumer in another package would need `pub`, which is never inferred — it becomes
`NoticeKind::Unreachable`. The moved module's own `mod` line is being relocated, so its
need is handed to `Surgery::relocate` instead of edited in place. Nothing is ever
narrowed, and a move that nobody outside needs changes no modifier.

**Grouped imports go back to the surgery.** An `ImportRef` inside `use a::{…}`
carries an `ImportGroup` (prefix, item span, list, statement). `Rebase` collects such
entries per statement and calls `Surgery::regroup`, which rewrites an entry in place
when its target stays under the group's prefix and otherwise moves it out into its own
statement. Whatever a surgery returns as `skipped` becomes a `Notice` with the text it
_would_ have written — reported, never silently dropped.

**Ids are content-derived.** `MatchId = blake3(path, span, text)[..12]`. A selection
made from one process is valid in the next as long as the file is unchanged, and
becomes an explicit error otherwise — the same guarantee `Plan` fingerprints give at
apply time. `Selection::Ordinals` is the human-facing equivalent: 1-based positions
in the search's result order, which the engine fixes (declarations first, then path
and position) so the numbers a preview prints are the numbers `--select` reads.

## Recipes

### Add a language

1. `crates/vvv-lang/Cargo.toml`: a feature `<x> = ["ast-grep-language/tree-sitter-<x>"]`.
2. `src/<x>/grammar.rs`: one `const GRAMMAR: Grammar` with the `SYMBOLS`, `IDENTIFIERS`
   and `IMPORTS` tables. Use a probe program (`lang.ast_grep(src).root().dfs()`) to
   discover node kinds and field names.
3. `src/<x>/mod.rs`: a type alias `pub type X = AstGrepLanguage<ast_grep_language::X>;`
   and an `impl X { const ID; fn new() }` calling `AstGrepLanguage::describe(ID,
   extensions, grammar, GRAMMAR, &SEMANTICS)`, plus `.with_layout(…)` and
   `.with_surgery(…)` when the language's paths can be followed. No `Language` impl to
   write. Declare the module in `lib.rs` under
   `#[cfg(feature = "<x>")]`. Tests: one snippet exercising every rule, one for
   `references`, one for `imports`.
4. `src/<x>/layout.rs`: implement `Layout` — `address` from the path and the project's
   packages, `resolve` for the path forms the language has (existence is a lookup in
   `project.files`), `manifests`/`package` if the language has packages — which is what
   scope-aware `rename` needs. `src/<x>/surgery.rs`: implement `Surgery` — `render`
   preserving the original's style; for `move`, `relocate` for side effects (from the
   parsed files `Layout::touched_by_move` named), `companions` on the layout for files
   that travel together, `regroup` if the language groups imports. Test both against a
   `syntax::fixture::Fixture` — files as text, no `Vfs`.
5. `crates/vvv/Cargo.toml`: feature `<x> = ["dep:vvv-lang", "vvv-lang/<x>"]`; register the
   type in `languages.rs`'s `Builtins`; add to default features if it should ship by
   default. Add the isolation build and test to CI's `features` job.

### Add a mutating command

1. `vvv-engine/src/protocol/intent/<cmd>.rs`: the intent as serde data with builder
   methods; `protocol/result.rs`: the answer (`intent`, `applied`, `history_id`,
   `files`, whatever else the command explains), `impl Mutation` for it; add the
   `Request` and `Answer` variants in `protocol/request.rs`.
2. `vvv-engine/src/<cmd>/`: the components the command needs (a noun with state that
   answers questions — see `Rebase`, `Target`), and `impl Command for <Cmd>Intent`
   whose `run` asks the graph, builds a `Change` and ends in
   `Planned::of(cx.workspace, change, |bound, files| Cmd { … })`. Test with a fake
   language in `vvv-engine/tests/<cmd>.rs`.
3. `crates/vvv/src/cli/commands/<cmd>.rs`: `clap::Args` struct, `run(self, &Context)`
   (build a `Request` and `ctx.run(request)`); add it to `Commands`. The answer's
   blocks go in `output/view/` — a builder in `lines.rs` if it is a row, a function
   in `read.rs`/`change.rs` and an arm in `view::of` otherwise. A mutation's `apply`
   flag rides on the `Request`.
4. Update `protocol.md`.
