# Working on vvv

Read `docs/architecture.md` before designing anything. It states the current shape and
the rules that keep the crates apart.

## Commands

```console
cargo build                                   # default features: rust, typescript
cargo test --workspace --all-features --no-fail-fast    # the corpus gate is in here
cargo clippy --workspace --all-targets --all-features
cargo fmt --all
dprint check                                  # markdown, JSON and TOML
cargo doc --workspace --no-deps
cargo run -- search --symbol trait            # dogfood on this repo
```

CI (`.github/workflows/ci.yml`) runs these with `RUSTFLAGS=-D warnings` and
`RUSTDOCFLAGS=-D warnings`, plus the feature matrix; `dprint` runs on Linux. Run them
before declaring work done, with the same flags:

```console
export RUSTFLAGS="-D warnings"
cargo build -p vvv-rs --no-default-features                  # the CLI with no languages
cargo build -p vvv-rs --no-default-features --features rust
cargo build -p vvv-rs --no-default-features --features typescript
cargo test -p vvv-lang --no-default-features --features rust
cargo test -p vvv-lang --no-default-features --features typescript
cargo test -p vvv-core -p vvv-engine --no-default-features
```

A `v*` tag runs `.github/workflows/release.yml`: `git-cliff` builds the notes from
`cliff.toml` and the GitHub Release is created. crates.io is published by hand, not by
CI: `git-cliff -o CHANGELOG.md`, then `cargo publish --workspace`.

## Layout

```
crates/
  vvv-core      the plugin contract: the nouns, the Language/Layout/Surgery traits, what
                they take and return; no parser, no I/O, no lifecycle
  vvv-lang      syntax/ (the only code that touches ast-grep nodes) + one module per
                language behind a feature: grammar + semantics tables, a Layout, a Surgery
  vvv-engine    the façade: Engine, the plan lifecycle, the workspace and its Vfs,
                history, and protocol/ — every type --json prints, the contract for AI
                tools. Takes a LanguageRegistry; names no language
  vvv-tui       the picker: Tui::new(engine).editor(..).color(..).run(); links the engine only
  vvv           the entrypoint: clap consumer of vvv-engine (+ vvv-tui behind `tui`);
                cli/commands/ map 1:1 to intents; languages.rs is the composition root
                that names vvv-lang and picks the plugins the build ships
                (package `vvv-rs` — `vvv` is taken on crates.io — binary `vvv`)
docs/          architecture, guide, protocol, report
```

## Rules

- **An engine runs commands.** `Engine` has `new`, `run`, `root` and
  `language_ids`; every capability is a request type in `protocol/` with an
  `impl Command` next to its components. A new method on `Engine` is the wrong place
  for anything.
- **Library first.** Behaviour lives in `vvv-engine`. `crates/vvv` builds an intent,
  runs it, runs `Apply` if asked, hands the result to a `Reporter`. Nothing else —
  no `format!` of user-facing text outside `output/`. What crosses a boundary is data
  (`Intent`, `NoticeKind`, error variants, protocol types); words are the display
  layer's job. An error or notice that only exists as a `String` is a bug.
- **Interface logic sees the engine; the composition root names plugins.**
  `crates/vvv` and `vvv-tui` depend on `vvv-engine` and nothing below it, except
  `crates/vvv/src/languages.rs`, the composition root: it builds the `LanguageRegistry`
  the engine runs on and does nothing else with a language.
  `grep -rn "use vvv_core\|use vvv_lang" crates/vvv/src crates/vvv-tui/src`, excluding
  `languages.rs`, must find nothing. An interface that wants the workspace has found a
  missing engine method; one that wants a language plugin has found the composition
  root. One membership test per crate: core — does a plugin need it to be one?; engine —
  does it act on a tree, or cross to a client as data (then `protocol/`, which takes no
  `Workspace`)?
- **The report is the result; the view is how it is shown.** `vvv-engine::report`
  composes an `Answer` into a `Document` — the facts every interface can read, with
  no layout — and a `View` lays it out as a `Presentation` of rows (`Detailed` is
  the terminal's; the picker holds `Compact`). `crates/vvv/src/output/render/` only
  styles what a view produced, so it names no command:
  `grep -rn "Answer\|Match\|Occurrence\|Search\|Rename\|Move"
  crates/vvv/src/output/render/` must find nothing. The report is never serialized:
  `--json` is the `Answer`. A row an interface can act on carries a
  `Site { path, line }`.
- **The plugin boundary is data.** `Language` methods take `&str` and return plain
  serializable values (`RawMatch`, `Symbol`). Only `vvv-lang/src/syntax/` imports
  `ast_grep_core`; language modules contribute `Grammar`/`Semantics` tables, a pure
  `Layout` and a pure `Surgery`, never traversal code and never I/O.
  `grep -rn Workspace crates/vvv-lang/src` must find nothing.
- **No crate without a reason.** A crate exists to keep a dependency out of something
  (core vs parsers), to be a separately consumed unit (the binary, the protocol), or to
  version independently. Otherwise it is a module. The test is in `docs/architecture.md`.
- **Components and capabilities, not steps.** A type is named for what
  it is and what it can answer — `Graph`, `Namespace`, `Candidate`, `Scope`, `Target`, `Rebase`,
  `Extraction`, `Widen`, `SymbolMove`,
  `Plan` — never for the step it performs. Behaviour hangs off the type that owns the
  data; sequences live in the `Engine` method as plain code short enough to read as a
  sentence. A trait exists once two real implementations answer its question; with one
  it is a struct. A struct named with a verb, or a method whose only input is the
  previous step's output, is the smell. A helper with no natural owner is a sign the
  type is missing.
- **Nothing writes without a `Plan`.** Planners emit a `ChangeSet`; `Plan::preview` is
  read-only; `Plan::apply(self)` consumes the plan, checks fingerprints, and returns a
  `Receipt`. `ChangeSet` has no write method on purpose. A command returns
  `Planned<T>` — its wire result with the plan beside it — and only the `Apply` command
  turns that into writes and a history entry.
- **Every engine feature is testable with a fake language.** `vvv-engine/tests/` must
  keep passing with `--no-default-features`. If a test needs a real grammar it belongs
  in the language crate.
- **A workspace path is a `RelPath`; an OS path is a `PathBuf`.** `RelPath` is a
  project path — `/` on every platform, like a `mod` path or a `crate::a::b` address —
  and is what goes on the wire, into a row, or into a diagnostic. It derefs to `Path`,
  so it reads as one, but it cannot spell a separator the host's way. An OS path is
  made from one at the edge (`Workspace::absolute`), where a file is actually read; a
  `PathBuf` that reaches a row or the wire is the bug this prevents.
- **Selection is the answer to ambiguity.** Syntactic resolution shows every candidate
  with a stable `MatchId`; humans and agents narrow with `--select`. Do not add
  heuristics that silently drop candidates.
- **Dependencies:** an external crate goes in the root `[workspace.dependencies]` only
  when two or more crates use it; otherwise it is declared in the crate that uses it.
- **A `lib.rs` is a surface.** Crate doc, `mod` lines, `pub use` lines, nothing else;
  a type declared in `lib.rs` is in the wrong file (`engine.rs` holds `Engine`,
  `tui.rs` holds `Tui`).
- **A public error enum lives in `error.rs` at the crate root** (`EngineError`,
  `vvv_tui::Error`). Errors a component raises stay with that component.
- **Edition 2024, `unsafe_code = "forbid"`, clippy `all` as warnings, CI `-D warnings`.**

## When you change something

- User-visible behaviour changed (a flag, a key, what a command rewrites) → update
  `docs/guide.md`; the README stays a front door and only changes for headline features.
- JSON output changed → update `docs/protocol.md` in the same change.
- Human output changed → the `insta` snapshots under
  `crates/vvv/src/output/human/snapshots/`
  (CLI) or `crates/vvv-tui/src/snapshots/` (TUI) change too; review them, then
  `INSTA_UPDATE=always cargo test -p vvv-rs` (or `-p vvv-tui`) to accept. Never
  `println!` in a reporter: write to its `out`/`err` so tests can capture it.
- What a command _means_ changed (a verdict, an address, an edit) → the corpus gate
  changes: `crates/vvv/tests/corpus.rs` runs every command over the two workspaces
  under `crates/vvv/tests/corpus/` and keeps the exact output in
  `crates/vvv/tests/corpus/snapshots/`. Read the diff as the review of the change —
  every line that moved is a behaviour that moved — then accept with
  `INSTA_UPDATE=always cargo test -p vvv-rs --test corpus`. A new shape of code vvv
  should handle goes into the corpus first, with a case that shows it. The same file
  checks three properties every mutation must keep (apply is the preview, undo is the
  identity, a batch is composition); those never get accepted, only fixed.
- TUI change → `update`/`on_event` stay pure (no I/O, no time); views stay pure
  functions of the model; the engine is only ever called from `worker.rs`; the terminal
  and the editor only from `tui.rs`; the surface stays `Tui`'s four methods. Test with
  `Action`s and `Event`s, then a `TestBackend` snapshot.
- New language → a feature and a module in `vvv-lang`, not a crate: `grammar.rs`
  (`GRAMMAR`, `SEMANTICS`), `layout.rs`, `surgery.rs`, a type alias over
  `AstGrepLanguage`. Never a hand-written `Language` impl.
- New engine test → use `tests/common/mod.rs`'s `Fake` language; extend it rather than
  writing another fake.
- New test → compare paths by component, never by a `display()` string; a message that
  embeds a path is asserted with `\` normalized to `/`. CI runs Windows, where a joined
  path spells its separator as `\`; `Path`'s `PartialEq` is component-wise, so
  `Path::new("a/b")` equals `Path::new("a\\b")` there.
- Commit only when asked.
