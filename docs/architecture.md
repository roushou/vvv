# Architecture

A library with thin interfaces around it. Dependencies point inward; nothing below
knows about anything above, and the interfaces know only the engine — with one
exception: the CLI's composition root names the plugins the engine is built with.

```
crates/vvv   vvv-tui     the interfaces: CLI, picker
      │        │             the CLI's composition root (`languages.rs`) also names plugins
    vvv-engine            Engine: one entry point, `run`; capability-owned and shared wire data
         │
     vvv-lang             syntax/ (ast-grep adapter) + rust/, typescript/ behind features
         │
     vvv-core             the plugin contract: nouns + traits (no parser, no I/O)
```

One membership test per crate: core — _does a language plugin need it
to be one?_; lang — _is it a grammar or the ast-grep adapter?_; engine — _does it do
something to a tree, or cross to a client as data?_; the interfaces — _is it a
rendering or a keystroke?_ Within the engine, shared wire data lives in `protocol/`;
capability-specific wire data may live beside the command and report that use it.
The data and serialization code do not access `Workspace`. A type lives in the
lowest crate whose test it passes, never lower because a consumer could not
otherwise reach it.

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
| `resolve`   | `Address` (a `PackageId` + path; nothing holds across packages), `Packages` (members and their dependencies, renames included), `Project`, `Layout` (addresses and path resolution), `Surgery` (edit spelling), `SideEdit`                                        |
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
build or a language server to ask — and one wire dispatcher, `Engine::run(Request)`.
The dispatcher holds a shared operation guard and delegates to capability-owned
execution bodies. It acquires the graph only for source-tree questions and
planning. History, undo, retained-plan apply, and file preview do not refresh the
tree. The engine owns orchestration, not capability behavior.

Typed clients call `SearchQuery::execute(&Engine)`, the other queries' `execute`,
mutation intents' and `RewriteOf`'s `plan`, `Apply<T>::apply`, or
`Ledger::new(&Engine).history()` / `.undo()`. These keep concrete outputs such as
`Search`, `Planned<Rename>`, and `Applied<Rename>`. `SearchQuery` wraps the plugin's
`Query` without changing its serialization. Execution bodies take the graph,
workspace, or registry they use explicitly; there is no `Command` trait or engine
`Context`. Public typed methods acquire the same operation guard as the dispatcher;
internal bodies do not reacquire it.

`Engine::run` returns an in-process `Execution`: `Completed(Answer)`,
`Preview(Planned<MutationAnswer>)`, or `Applied(Applied<MutationAnswer>)`. Only
mutation previews retain executable plans. `into_preview` and `into_applied`
reject a mismatched kind with a structured error; `into_answer` consumes the handle
at a reporting or wire boundary. `Execution` is not serialized. The CLI and serve
convert it to the existing `Answer`; the picker retains previews and applied
completions until it has extracted what its view needs.

A mutation answers with `Planned<T>`: immutable presentation data beside plans
and the captured history intent. `Apply` requires the sealed `Mutation` capability,
writes the plans, records history, and returns `Applied<T>` with a required history
id. Queries cannot carry `Planned` or reach `Apply`. Typed plans and completions can
widen to the closed `MutationAnswer` sum without losing their handles; they become
`Answer` only at the reporting boundary. There is no arbitrary result mapping or
mutable result access. Mutation payloads own a `MutationState`: `Preview` or
`Applied { history_id }`, serialized as the existing `applied` and `history_id`
fields. Contradictory states are rejected during deserialization.

`Intent` remains a mutation description recorded by history and composed by batch.
Its `into_request(apply)` conversion adds execution policy as data; Intent does not
execute or dispatch capabilities. A batch runs preview requests on an independent
staging engine, extracts their executable previews, and applies the retained plans
to the overlay. Real writes and history still belong to one outer apply. Rename
contains the `ReferencesQuery` it asks, flattened on the wire.

One operation mutex spans planning, application, history save, and recovery; it is
acquired before the graph mutex. Engine clones share both locks and dirty state.
Apply and undo mark dirty state before attempting file effects, including
unsuccessful operations. External `Engine::touched` marks it without acquiring the
graph; the next graph access consumes it. This expires the trusted walk without
clearing cached candidates: a session reads contents only when stamps changed. A
touch arriving during refresh remains pending for the next access.

A capability module owns its request and answer data, typed execution, and report
composition. Related queries share a module when they describe the same concepts:
declarations (outline and where), imports (deps, explain, and diagnostics), and
usage (impact and dead); search, file preview, and surface have their own modules.
The [command ownership index](../crates/vvv-engine/src/capabilities/mod.rs) maps
every command to its owner. Keep it current when a command is added or moved. `protocol/` keeps shared wire types and the central `Request`/`Answer`
contract. Capability modules are private; the canonical public paths for the
mutation types are `vvv_engine::Rename`, `vvv_engine::RenameIntent`,
`vvv_engine::Move`, `vvv_engine::MoveIntent`, `vvv_engine::MoveSymbol`, and
`vvv_engine::MoveSymbolIntent`. The crate root re-exports these directly from their
owning modules; `protocol` provides no aliases for them. Data and serialization
code do not access `Workspace`.

Deferred API work: query types retain both crate-root and `protocol::` public
paths for compatibility, while the six mutation types above are root-only.
Unify this policy in a separate API commit; structural moves preserve both query
paths and do not restore mutation aliases.

Each `Candidate` (a file with its language) answers
`find`, `references`, and `imports` for itself, and parses once however many
questions it is asked: its `Facts` are computed on first use and shared. Per-file
work runs in parallel with `rayon` and collects in path order.

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
76k-file tree, ~230 ms), and the engine's own writes (`Apply`, `Ledger::undo`) or
`Engine::touched()` (the TUI after an editor hand-off) end the trust at once.
One command uses one graph however many questions it asks — a
rename's declarations, its other declarations, its occurrences and its project all come
from the same walk.

A file's **`Fragment`** (`graph/fragment.rs`) is its structural edges held as data:
its module address, every addressable declaration placed (`Declared`: symbol, address,
reach) and every import statement and qualified path resolved (`Edge`: the `ImportRef`
and the address the layout gave it, or the address another import's binding leads to
when the path's head is a name the file imports). An edge retains whether that
address came directly from the layout or through a particular imported binding;
the immediate binding links back to its edge, retaining provenance across an alias
chain. Same-file bindings propagate to a fixed point, independent of import order;
unseeded cycles stay unresolved. Scope consumes those completed resolutions rather
than following an additional alias hop. Move planners consume these edges,
including for import provisioning and destination cleanup. `Rebase` transforms
resolved addresses and preserves an alias spelling when rebasing its binding
already supplies the required target; it does not resolve raw source paths again. A candidate builds it once per
(file stamp, project build) and keeps it, so a session that asks ten whole-tree
questions resolves each file once. The retained `Arc<Project>` identifies the build:
equal projects reuse it, even across refreshes that ask no project questions;
a changed project gets a new identity and invalidates every fragment and scope.
A file's **`Scope`** — what it
sees: bound names, opened modules, resolved and unresolved paths — is read off the
fragment and kept beside it, so `references` judges tokens per name through a lookup;
only a token in the middle of a path costs a resolution of its prefix. `aliases_of`
reads only files spelling one of the names found so far, or — for glob re-exports —
the language's `glob_marker` (`::*` in Rust) in the alias's own package or naming it.

| command            | what it asks                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SearchQuery`      | `Graph::search`: `containing(literals)`, then each `Candidate::find(query)`, in parallel; declarations get their address from the namespace and move to the front                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `RewriteIntent`    | `search`, `Selection::narrow`, one `Edit` per match from the `Template`, `Plan::new`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `RenameIntent`     | declarations by name (narrowed by `declared_in`); per declaring language: a `Target` (module address + name, via the language's `Layout`) when one path-addressable declaration is meant — two refuse and ask for `declared_in` — then `Graph::containing(name)` and `Target::judge` on each file (each token judged by a `Scope` built from the file's imports; a token ending a path is judged by the path, or `?` when its head is unknown); default selection by confidence; `Selection::narrow`, one `Edit` per occurrence, `Plan::new`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `MoveIntent`       | validate paths and language, `Graph::namespace_of` (layout, surgery, project in one), compute the `MoveSet` (a file, or every file under a directory, plus the layout's companions such as Rust's `a.rs` ↔ `a/`), then `Rebase::rewrite` on sites from every `Graph::fragments(ns)` (re-render every import resolving under the old address, moved files' own imports from their new locations — each a snapshot-bound `FileRewrite`, converted to a `Change`), `Reachability::check` on the references it touched, a `Widen` per violation (an edit, or a notice across a package boundary), the surgery's side edits (`relocate`, told what the moved `mod` line needs), record every move, `Planned::of`                                                                                                                                                                                                                                                                                                                                                                |
| `MoveSymbolIntent` | the graph establishes the situation — the `Extraction` (the declaration and its pieces; `impl` blocks are `Impl` symbols named after their type), both files parsed, `Graph::consumers` of the old address, `Graph::references` for the old file's remaining uses — then `SymbolMove` runs its operations over it, each appending to one `Change`: the old file's imports and siblings the text names become imports in the new file (siblings `Widen`ed if needed); `Site::partition` assigns source edges to moving and staying text before `Rebase` transforms and renders each selected edge once in its final context, and rewrites other consumers from their own sites; a bare use left in the old file imports it back; an import of it in the new file is deleted; the declaration is `Widen`ed for consumers its reach at the new module no longer admits; last, the cut (`Extraction::cuts`) and the paste (`Extraction::assemble`, which carries only edits computed for the moving site) at `Surgery::item_insertion`, imports at `Surgery::import_insertion` |
| `BatchIntent`      | `Workspace::staged()` (an `Overlay` the real files never see); each intent planned by an engine over it and applied to it, receipts chained with `Receipt::then`; the preview is every touched file now against the staging tree at its final path                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `Request`          | one match in `Engine::run`, delegating to typed capability bodies and returning `Execution`; mutation previews retain plans, applied requests commit them through `Apply`; `into_answer` consumes the result at the presentation boundary                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `Apply(planned)`   | validate the history snapshot and next id, then apply every plan through one `Transaction`; retain effects and receipts until saving the ledger succeeds; a file or ledger failure recovers the full before-state; the result comes back with `applied` and `history_id` set                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `Ledger::undo`     | one validated history snapshot; `Receipt::undo_in` checks fingerprints, restores files and cleans owned empty directories through one `Transaction`; save the snapshot without its newest entry before releasing recovery effects; on failure, recover the pre-undo state; answers `Undo` with what was restored                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `Ledger::history`  | `Ledger` reads a validated snapshot and returns each record's entry                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `SurfaceQuery`     | every fragment's declarations in the package; each public one, or one `aliases_of` offers elsewhere, listed with its aliases and how many other fragments import any of its addresses                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `ImpactQuery`      | `references` for the declaration, `aliases_of` for its addresses, then breadth first over fragments: a module whose imports lead under a frontier address joins the next ring, once, at the depth it is first reached                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `DeadQuery`        | for each placed declaration (a type and its impls once), `references(name declared_in file)`: no `Resolved` token beyond its own name spans means unreferenced, `Unresolved` tokens are counted as `unsure`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `ImportsQuery`     | each fragment's declared imports: no address is unresolved; the same (address, glob) twice is redundant; a binding (`ImportRef::binding`, alias or last segment) no token outside the statement spells is unused, skipped for re-exports and for languages whose imports hide which names they take; files with no module address are `unplaced`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `FileQuery`        | the file loaded, `Language::highlights` from the language claiming it                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

The engine's supporting modules:

| module                                                                                          | holds                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vfs`                                                                                           | `Vfs` trait (read, write, walk, `stamp`); `MemoryVfs` for tests, `DiskVfs` (`.gitignore`-aware parallel walk), `Overlay` (writes over a base that is never touched — how plans compose)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `workspace`                                                                                     | `Workspace` = root + `Arc<dyn Vfs>`; `SourceFile`; relative/absolute path handling                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `change`                                                                                        | `Change`: what an operation proposes before it is bound to the tree — edits by file, files moved, notices, respellings. Every step of a mutation (a `Rebase` rewrite, a widening, a relocation) answers with one; the command merges them and `Planned::of` binds the result to a `ChangeSet` (overlaps refused there, once), previews it and keeps notices and respellings for the answer                                                                                                                                                                                                                                                                                                                    |
| `plan`                                                                                          | `Plan` (change set + fingerprints) → `preview` / `apply` → `Receipt` (with post-apply fingerprints) → `rollback` / `undo`; `Planned<T>`, a result with its plans and preview, `Deref` to the result                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `protocol`                                                                                      | shared wire types, never tree access: `Match`/`MatchId`/`Occurrence` with `Confidence` and `Reason`, `Selection`, the answers (`Outline`, `Deps`, `Explanation`, `References`, `Locations`, `File`), `Notice`, `Respelling`, `Reach`, `Template`, `HistoryEntry`, `FileChange` with a `Diff` (its `Hunk`s read from `similar`, rendered to the wire string), `SCHEMA` and the `Response` envelope, `Request`/`Answer` (every command as one value and every result as one), `Call`/`Reply` (a request with an `id` and its reply, what `vvv serve` speaks), `Failure` with its `ErrorCode` (what an error is on the wire; `EngineError::code()` and `::hint()` say which), `display` — the shared styled-line |
| IR (`Role`/`Piece`/`Line`, `hit`, `diff`, `counts`) each interface renders in its own colours — |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| and `vocabulary` — how the answers are read (below). Documented in [protocol.md](protocol.md)   |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |

`Apply` validates and retains the history ledger before attempting any file write,
and computes its next id with checked arithmetic. An unreadable ledger prevents
apply without changing files. Its validated `HistorySnapshot` owns the entries and
the ledger's exact pre-apply contents (or absence). Saving the new ledger is a file
effect in the same transaction as the edits and moves; receipts and recovery effects
remain live until that save succeeds. A failed or partial ledger write restores
the original ledger bytes, or removes a newly created ledger, as well as recovering
the files and owned empty directories. Ledger restoration failures use the same
structured recovery result and name `.vvv/history.json` explicitly.

`Vfs::move_if_absent` never replaces an occupied destination, including during
recovery. The caller prepares parents explicitly so their creation stays in the
transaction's effect log. A failed move returns a `MoveError` with a `MoveState`:
unchanged, moved, destination linked with source removal incomplete, or unknown.
An unchanged failure does not acquire the destination; recovery preserves a racing
creator even if its contents equal the source. Recovery never acquires or removes a
destination whose acquisition is unknown; it reports any differences that remain.
Disk errors can confirm a completed move by the retained source file handle at
the destination, rather than guessing from equal contents.

Disk moves use atomic no-replace renames: rustix's `renameat2(RENAME_NOREPLACE)`
on Linux and `renamex_np(RENAME_EXCL)` on macOS. Windows uses the safe
`atomicwrites::move_atomic` wrapper over `MoveFileExW` without replacement or
cross-volume copy flags. Same-volume local renames are a single native operation;
remote filesystem errors can leave an uncertain outcome. There is no cross-volume
copy fallback. Linux/macOS fall back to `hard_link` then `remove_file` only for
`ENOSYS`, `EOPNOTSUPP`/`ENOTSUP`, or `EINVAL` (unsupported rename flags).
Other platforms fall back only on an unsupported-operation error. The fallback
preserves the destination but is not atomic: an unlink failure leaves both names
and reports that effect. Filesystems without hard links fail without replacing
the destination. Memory and overlay moves acquire their state lock for the
destination check and mutation; overlay moves never write through to the base.
The overlay isolates its own mutations, not independent changes to its base.

Case-only file moves admit a destination only when `Vfs::same_entry` proves
that both names address one directory entry. Separate hard links are occupied
destinations even when their inode identity matches. `entry_path` returns stored
spelling, and `names_alias` lets the overlay apply the base's naming policy to
new staged files too. Disk entry lookup compares actual directory entries and
file handles; for absent staged names it observes an existing cased entry in the
directory (or nearest existing ancestor), without writing a probe.

The transaction routes a case-only move through a unique hidden name in the source
directory. Both destination-preserving legs enter the effect log before I/O,
including retry attempts whose temporary destination was occupied. It retains the
initial stored spelling and observes it during recovery, so equal contents alone
cannot hide a failed restoration of case. Temporary files that cannot be restored
appear explicitly in `Recovery.remaining`. This logical two-leg operation is not
atomic, even when each primitive rename is atomic. Receipts record the logical
source and destination, and receipt rollback uses the same transaction and case
handling. Undo retains its transaction through saving the ledger without the newest entry.
A failed file restoration, directory cleanup, or history save compensates toward
the pre-undo file and ledger state, with structured recovery failures if that state
cannot be restored or verified.

CI tests run on Linux, macOS, and Windows; formatting, documentation, lint and
feature-matrix gates run on Linux. Local validation is on the host platform.

`history.rs` is the undo stack: `.vvv/history.json`, newest last, capped at 20 because a
receipt carries full pre-apply file contents. Each record stores the `Intent` that was
applied — data, never a sentence — and its receipt; the receipt never leaves the engine,
a client sees the `HistoryEntry` (id, time, intent, what was written). It goes through the
`Vfs` like everything else, so engine tests exercise it in memory.

New receipts also retain the directories actually created by the file plans, in
creation order; `Receipt::then` keeps that ownership across batch steps. Ledger-only
parent directories are excluded from the receipt. Undo removes owned directories
in reverse creation order only when empty. Pre-existing directories and directories
containing other files are retained. Old receipts have no directory ownership
evidence and conservatively retain their directories. A removed directory has a
before-state in the undo transaction; recovery recreates parents before children
through `Vfs::create_dir`, which does not replace an occupied entry.
The directory-capable disk backend implements this recovery primitive; file-only
backends return an explicit unsupported-operation error if asked to create one.

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

`report/` is the result as a document. `Document::of(&Answer)` retains the facts
needed by every view, without presentation options. Source-bearing blocks retain
matches or sites; outline and dependency blocks also retain their owning file
path. Reference verdicts retain a `ReferencePlan` with files and mutation state,
even when a view hides its patch. Per-answer constructors live on `Document`,
implemented beside execution in the capability's owner (see the command ownership
index). References shares rename's module; rewrite, batch, history, and undo
composition live in their existing execution modules. File preview still has no
rendered document.

The shared report modules are `document.rs` (construction, shared helpers, and
`of`/`error` delegation), `block.rs` (structured blocks, notes, and reference plan
data), `row.rs` (rows and source sites), `view.rs` (View, Options, Presentation, and
Detailed), and `lines.rs` (shared line builders). `report/mod.rs` re-exports the
same public vocabulary; capability-specific summaries stay with composition.

`View::present` turns those blocks into a `Presentation` of rows. `Options` belongs
to this boundary: the view chooses collapsed or expanded verdicts, reach details,
and which patches to display. Shared line builders do not carry expansion flags or
make verbose/diff decisions. The CLI's `TerminalView` delegates detailed layout and
adds flag advice to the presentation; the picker receives the shared document
without CLI instructions. A document can be presented again with different options
without recomposition.

Rows for declarations, outlines, imports, explanations, references, respellings,
and notices preserve their reported `Source { path, line }`. Summaries, separators,
and suggested imports carry no source. Diff rows use structured hunk coordinates:
preview rows refer to old-side context and removed lines; applied rows refer to
new-side context and added lines, at the moved destination when present. A line
that does not exist on that side is not actionable. Diff headers are metadata.
Neither document nor presentation is serialized: `--json` remains the `Answer`.

### `crates/vvv` — the entrypoint

Package `vvv-rs` (the bare name is taken on crates.io), binary `vvv`.

`Cli` (global `-C`, `--json`, `--color`) → `Context { engine, format }` (`Engine::new`)
→ `cli::commands::*Cmd` (`clap::Args`; fields are the command's own inputs;
`run(self, &Context)`) → a `Reporter` (`Human` or `Json`). A command builds a
`Request`; `Context::run` runs it, consumes `Execution` into `Answer`, and hands it to the reporter: three lines,
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
- `human/advice.rs` — `TerminalView` adds the CLI's flag hints to the presentation
  after delegating layout to `Detailed`; no engine report composition names a CLI flag.
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
- `model.rs` — all state as data: the `Search` hub (query, results, context; a
  declaration under the cursor can be entered as its `subject`, and the hub then shows
  one read-only relation about it — references, impact, definition, deps), the
  current `Mode` (`Rename`, `Move`, `Rewrite`, `History`, each with its input, its
  rows, its panel cursors and a focus enum implementing `Panels`), an optional
  `Overlay` (menu, confirm, help), status.
- `action.rs` — `Action` (what the user did, generic across modes: `Input`, `Enter`,
  `Toggle`, `FocusNth`…), `Effect` (what to ask the engine: `Search`, `Query` for a
  read-only request, `Plan`, `Commit`, `Preview`, `History`, `Undo`; `Edit` for the
  loop itself), `Event` (what came back; `Planned` carries what a mode shows about its
  intent). Plain enums.
- `keymap/` — the key vocabulary, pure. `keys.rs`: `Key` (a `Code` and
  `Modifiers`) and its constructors. `mod.rs`: `Trigger` is what a
  `Keybinding` listens for (`Key`, `Text`, `Any`), `Dispatch` is what it does
  (`Run(A)`, `Type`), `Legend`/`Bar` say how it reads, and `Layer` is a named
  set of bindings with `resolve` and `rows`. `Key::from_event` in `keys.rs`
  turns a crossterm event into a `Key`, folding shift into the character.
- `screen/` — shared key, focus, and help metadata (`Screen`, `Panel`) and
  the application frame. `BoundScreen<V>` owns a typed view and its layout and panel
  callbacks; panels render from that view without inspecting `Mode`.
  `screen/defaults.rs` holds the shared key layers. Rename, moves, rewrite, and history now own their state,
  transitions, metadata, and typed views under `modes/rename/`, `modes/moves/`,
  `modes/rewrite/`, and `modes/history/`;
  search and overlays use a
  temporary `LegacyScreen` renderer until their individual migrations.
- `modes/context.rs` — shared status borrowed by a mode transition, without access
  to `Model` or another mode. `input.rs` holds `TextInput`, which edits a borrowed
  string buffer for name, destination, and template inputs.
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
  debounced searches and plans, the editor hand-off, teardown. It supplies the frame
  timestamp used by `HistoryView`; views do not read the clock.
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

**Plans authorize file writes.** `plan/mod.rs` holds Plan's preconditions,
staging, preview, and its thin apply entry point. `plan/transaction.rs` owns
attempted effects, before-states, and recovery; `plan/receipt.rs` owns applied
receipts, their composition, and undo validation/restoration. `planned.rs` and
`fingerprint.rs` keep their existing responsibilities. Internal re-exports keep
the lifecycle's callers independent of these file locations.

`Plan` stages every file first (read, fingerprint
check, compute), then writes through a `Transaction`. Before a write or move is
attempted, the transaction retains its before-state and appends the effect to an
ordered recovery log. One transaction spans all plans of an apply, including a batch.
A successful `Receipt` describes the applied changes; it is not the recovery log
for a partially attempted operation. `ChangeSet` has no write method.

When a file operation or history save returns an error, recovery reverses the effect
log, continues restoring independent effects after an error, and checks every
retained before-state. Complete
verified restoration returns the initiating error. Otherwise `EngineError::Recovery`
reports the cause, failed restoration operations, confirmed remaining effects, and
paths whose state could not be verified. Recovery also removes owned empty parent
directories; `Vfs::prepare_parent` identifies the directories it actually created,
even when preparation fails partway through.

This is in-memory failure recovery, not crash consistency: there is no durable
journal, restart recovery, or isolation from external writers. Restoration concerns
file contents and locations and owned directories, not inode identity, timestamps,
or complete filesystem metadata. Apply and batch keep recovery effects and receipts
until their history entry is saved. Undo keeps its recovery effects until the entry is removed from the saved
ledger. On any returned file-operation, directory-cleanup, or history-save error,
the outcome is either verified restoration of the state before that command or a
structured recovery failure naming confirmed remaining effects and unverified
paths. Successful undo restores the receipt's file contents, locations, and case
spelling, removes its history entry, and removes owned empty directories.
Directories containing other files and directories without ownership evidence
are retained. These guarantees exclude crashes and concurrent-writer isolation;
they do not promise to restore inode identity, timestamps, or complete metadata.

**Plan provenance covers edited and moved files.** An immutable `SourceFile` gives
an edit producer a `SourceWitness` (relative path and content fingerprint). `Change`
requires that witness for edits and moves and refuses contributions from different
snapshots of the same file. Binding carries those observed fingerprints into `Plan`;
it never substitutes a fresh read for the source used to compute an edit. Preview and
apply compare the current contents with the observed snapshots before writing.
Relocation side edits use the snapshots handed to the surgery.

Rewrite expands captures from the candidate that supplied the matches. `RewriteOf`
revalidates selected retained matches and captures against a candidate before using
them; a resolved address added for reporting is not part of this source comparison.

Files only consulted during resolution are not witnessed by the plan. A manifest,
an unedited declaration, or another resolution input can change without making the
plan stale. Provenance protects the coordinates and contents of edited and moved
files; it is not a snapshot transaction over all resolution dependencies, nor does
it prevent external writes between staging and writing.

**Move destinations are preconditions.** A plan retains absence requirements for
every destination and checks all of them during preview and apply, before writing
any file. An occupied destination returns `exists`. This preflight is not an atomic
reservation: another process can still create a destination between the check and
`Vfs::rename`. Destination-preserving moves and their recovery outcomes belong to
the transaction work; the current Vfs move can replace a destination.

**Errors and notices are variants, not sentences.** `ResolveError` has one variant per
situation a layout can refuse (`Root`, `IntoItself`, `CrossProject`, `NoParentFile`
with the candidate files, …) and `Notice` carries a `NoticeKind`. `thiserror` gives
errors a canonical message; the CLI layers hints on top by matching variants, and JSON
clients get the fields, not the prose.

**Rename resolves through imports, not types.** `graph/scope.rs` builds, per file,
what its imports bring in: names (`use a::b::X`, grouped entries), opened modules
(globs, or any file import where the grammar hides the names), and qualified paths.
A token is `Resolved` if the file is the declaring module, imports the name, opens
the module, or spells a path resolving to the target; `Other` if any of those point
at a different same-named declaration; `Unresolved` otherwise. Methods, fields and
variants (`Semantics::is_addressable(kind) == false`) get no target and stay syntactic.
Ambiguity is per language: a Rust and a TypeScript `foo` never compete.

**Visibility is computed, never guessed**. After
`Rebase` has rewritten a move's paths it reports every reference it touched as (the
module it is read from afterwards, what it names afterwards). `capabilities/moves/reachability.rs`
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

**Extraction validates before slicing.** A symbol move retains the source snapshot
and checks its declaration extents for valid UTF-8 ranges and overlapping pieces.
Before rendering, `Site` partitions a symbol move's source edges: moving pieces
render from the destination, remaining source text from the source, and other
consumers from their own files. One `Rebase` transforms each selected resolved
address and renders it once. `FileRewrite` retains the observed source snapshot;
its conversion to a `Change` uses that snapshot's witness. Moving edits go directly
to extraction, without a second rewrite or a later partition of edits.

Assembly assigns every edit to one piece and rejects invalid, outside, or
overlapping edits as a conflict before producing a plan. Identical extents are
extracted once; coincident insertions keep their input order.

**Grouped imports go back to the surgery.** An `ImportRef` inside `use a::{…}`
carries an `ImportGroup` (prefix, item span, list, statement). `Rebase` builds a
`GroupedImports` request with unique entries from one statement, their required
resolved targets, and each prefix's meaning after planned prefix and binding
changes. A prefix edit covers its entries when their unchanged suffixes already
name those targets. Otherwise `Surgery::regroup` returns edits and one explicit
`RegroupedOutcome` per entry: its actual in-place replacement, a structural rewrite,
or skipped. The engine validates exact outcome coverage and in-place replacement
agreement before accepting edits. Skipped entries retain their requested targets
and become notices with the text the surgery would have rendered; no target is
inferred from edit spans or substituted with empty text.

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

1. Put the intent, answer, `impl Mutation`, typed `plan`, and report composition
   together in one engine capability module. Keep the data and serialization code
   independent of `Workspace`. Put reusable data in `protocol/`; re-export the
   capability's types at the crate root and add its variants to `Request` and `Answer`.
2. Keep the capability's components beside it (a noun with state that answers
   questions — see `Rebase`, `Target`). Its execution body asks the graph, builds a `Change`,
   and ends in `Planned::of(workspace, change, intent, |bound, files| Cmd { … })`.
   Add its Request routing to `Engine::run`; keep its behavior on its owning type.
   Test with the fake language in `vvv-engine/tests/`.
3. `crates/vvv/src/cli/commands/<cmd>.rs`: `clap::Args` struct, `run(self, &Context)`
   (build a `Request` and `ctx.run(request)`); add it to `Commands`. The answer's
   report composition goes in the capability module; shared rows stay in
   `vvv-engine/src/report/lines.rs`, and `Document::of` delegates to it. A
   mutation's `apply` flag rides on the `Request`.
4. Update `protocol.md`.
