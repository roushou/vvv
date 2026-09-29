# Architecture

A library with thin interfaces around it. Dependencies point inward; nothing below
knows about anything above, and the interfaces know only the engine — with one
exception: the CLI's composition root names the plugins the engine is built with.

```
crates/vvv ──▶ vvv-tui             CLI and picker
     │             │
     ├─────────────┴──▶ vvv-engine ──▶ vvv-core
     │                  capabilities,    plugin contract:
     │                  lifecycle, wire  data and traits
     └──▶ vvv-lang ──────────────────────▶ vvv-core
          languages.rs composes syntax/ and language plugins
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

## Type and implementation locality

A struct or enum, its constructors, invariant-preserving methods, and trait
implementations belong in the same file by default. Multiple `impl` blocks can
express different traits or bounds without requiring additional files. A reader
should be able to understand a type's state and behavior together.

Splitting implementations across files requires a strong, concrete benefit that
outweighs the extra navigation. A dependency boundary can require a separate
implementation; capability-specific report composition can stay beside the answer
it consumes so they change together. Neither method categories nor file length
alone justify a split. Separating serialization from tree access is a code ownership
constraint, not a requirement to separate a type from its execution methods.

The shared `Document` has capability-specific composition methods beside each
answer; its construction and shared methods live with its definition. The closed
mutation payload allowlist stays with `MutationAnswer` and its sealed trait so
executable result membership can be reviewed in one place. Cross-crate adapters
stay at the client boundary rather than adding interface dependencies to engine
or core types.

## Crates

### `vvv-core` — the plugin contract

Pure data and traits: what a language is given and what it hands back. No tree-sitter,
no file system, no lifecycle; `serde` and `thiserror` are its required dependencies.
The optional `schema` feature adds Schemars metadata beside serializable types. A
language's rule tables read alike: a `SymbolRule`, `ImportRule` or `HighlightRule` is
built by `new(..)` or a kind constructor and scoped by `under(kind)` (a direct
parent) or `within(kind)` (an ancestor).

| module       | holds                                                                                                                                                                                                                                                             |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `text`       | `Span` (byte range), `Position` (line/char column), `LineIndex`, `SourceText`                                                                                                                                                                                     |
| `paths`      | `Name` (one identifier), `ModulePath` (a path as spelled: a `PathHead` — package root, here, `n` up, `Self`, named, root — and segments) and `PathSyntax` (how a language spells one: `Scoped` `::` or `Posix` `/`; parses import text once, spells it back once) |
| `lang`       | `Language` trait, `LanguageId`, `LanguageRegistry`                                                                                                                                                                                                                |
| `oracle`     | `Oracle` trait (`refers(file, span) -> Option<Referent>`): a second opinion on a token from something that knows more than syntax; `Referent` (a declaration's file and name span)                                                                                |
| `symbol`     | `SymbolKind`, `Symbol` (name, node, extent, modifier), `SymbolRule` (the declarative plugin contract, with `leading` kinds and where the modifier is)                                                                                                             |
| `facts`      | `Facts`: everything about one file from one parse — symbols, imports, highlights, every identifier token interned                                                                                                                                                 |
| `navigation` | `LexicalBinding`, `BindingRule`, `BindingNamespace`, `NamedImport`, `NamedImportRule`: navigation facts and declarative extraction rules                                                                                                                          |
| `semantics`  | `Semantics`: what syntax means — path separator, import scoping, addressable kinds, visibility modifier → `ReachKind`                                                                                                                                             |
| `highlight`  | `HighlightKind`, `Highlight`, `HighlightRule`: syntax colouring as data                                                                                                                                                                                           |
| `import`     | `ImportRef` (a `ModulePath` at a span, grouped or not, declaring or a reference), `ImportRule`/`ImportGrammar` (where a grammar keeps import paths, which `PathSyntax` parses them, what re-exports and aliases look like, the text every glob spells)            |
| `resolve`    | `Address` (a `PackageId` + path; nothing holds across packages), `Packages` (members and their dependencies, renames included), `Project`, `Layout` (addresses and path resolution), `Surgery` (edit spelling), `SideEdit`                                        |
| `query`      | `Query` + `QueryBuilder`: structural (`pattern`, `kind`) and symbolic (`symbol`, `name`) halves                                                                                                                                                                   |
| `search`     | `RawMatch` (a match within one text, before it is tied to a file), `Capture`, `Role` (declaration / import / use, set by the searcher), `SearchError`                                                                                                             |
| `edit`       | `Edit`, `ChangeSet` (sorted, overlap-checked edits + file moves, no write method)                                                                                                                                                                                 |

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
`Search`, `Planned<Rename>`, and `Applied<Rename>`. `SearchQuery` flattens the plugin's
`Query` and adds an optional engine-owned `SearchScope`. Path component prefixes
and manifest-name/package-ID filters are intersected before match collection;
package membership uses the deepest layout-defined owning root. Empty scopes keep
the original wire shape. Execution bodies take the graph,
workspace, or registry they use explicitly. Public typed methods acquire the
same operation guard as the dispatcher; internal bodies do not reacquire it.

`NavigationQuery` resolves an exact, optionally versioned source occurrence. Its
capability owns typed requests/outcomes and report composition; `Graph` navigation
uses retained candidates, scope bindings, and a bounded re-export traversal rather
than collecting all references to every same-named declaration. Competing targets
remain candidates, narrowed with `Selection` or a versioned symbol request.

Search matches carry complete source content identities. Navigation replies pair
the target with its display container (an enum for a variant), full captured source,
highlights, and identifier anchors. Consulted source and manifest contents are
revalidated before returning; a detected change produces `StaleSource` and expires
the graph's trusted walk. Snapshot identity covers those inputs and the captured
file set, without promising isolation from external writers. Facts and scopes are
cached per existing graph rules; navigation replies have no persistent result cache.
`ResolutionQuery` exposes the same resolution through compact locations and evidence
without serializing preview files or declaration bodies. MCP navigation uses this
contract; CLI `navigate --compact` and wire `resolve` expose it to other clients.
This projection currently reuses full navigation internally.

Grammar tables declare lexical scopes, visibility start points, noncapturing item
boundaries, unsupported binding forms, and exact named-import rules. The syntax
adapter lowers these to plain `Facts`: eligible token spans, `LexicalBinding`s,
`NamedImport`s, and export restrictions. Navigation checks the innermost visible
binding before module lookup, keeps type and value namespaces separate, and uses
exact named bindings for TypeScript. Navigation-only declarations stay separate
from ordinary search/mutation symbols. Unsupported patterns and scope forms block
confirmation instead of falling through to a same-named outer declaration.

`NavigationQuery::execute_with` accepts a host-supplied `NavigationProvider` and
shared cancellation token. Syntax resolution runs first; only unresolved or
unsupported occurrences reach the provider. The provider returns its revision,
the exact origin, complete candidates, and versions of consulted workspace inputs.
Navigation validates these and target declarations, then revalidates sources and
provider revision before returning. Semantic evidence and snapshot identity include
the provider version. Cancellation is cooperative: the host must bound provider
work and avoid re-entering this engine while its operation guard is held. No
language-server process is managed here. The unversioned `Oracle` remains a
references capability input and is not semantic navigation evidence.

`ContextQuery` owns bounded context composition. It uses exact navigation to gather
a seed and directly referenced declarations. The enclosing declaration is a
versioned location by default; its body is an explicit `include_enclosing` opt-in.
Paged checkpoints retain this policy so an owner body cannot reappear through a
later relationship after it was omitted by policy. Optional
incoming scans inspect a bounded number of files and confirm same-spelling uses
through navigation; test-path evidence stays explicitly weaker than test coverage.
Navigation can record its consulted source/manifest versions for this compound
capability, which revalidates the whole set before returning. Result fitting counts
compact JSON bytes, preserves UTF-8 source boundaries, and reports omissions; it
never narrows an ambiguous candidate set to fit. Context does not broaden mutation
resolution or use the optional semantic provider.

`RelationshipsQuery` owns bounded caller, callee, and reference-site queries. Grammar
call rules lower call expressions to `CallSite` facts (callee span, syntactic kind,
and nearest named callable); anonymous callables stop ownership attribution. Value
navigation is enabled only in understood lexical scopes. Top-level functions in
the same file contribute declaration evidence even when the layout cannot address
an integration-test root; competing local and imported candidates remain explicit. Each relationship resolves
through the shared navigation engine; a called parameter or variable is an indirect
target, never an inferred function. Incoming candidates include the original spelling
and explicit local import bindings, so named aliases and re-exports can resolve
without a language server. Receiver types, indirect targets, and unenumerated aliases
remain explicit limitations. The capability captures and revalidates a fresh
`QuerySnapshot`, reports work/coverage limits, and publishes through `relationships`
and the thin MCP `vvv_relationships` tool. No relationship state or source inference
lives in an interface, and this query does not change mutation resolution.

`DiscoveryQuery` describes the build's commands, languages, and budgets without a
tree walk. `Call` retains wire data in `protocol`; its execution lives in
`capabilities/session.rs`, which enforces optional result budgets before returning
a reply. Budgeted mutation calls are rejected before dispatch. The CLI session only
parses lines, delegates execution, and serializes the response. These output limits
are separate from source-processing memory or time limits.

The optional engine `schema` feature generates Draft 2020-12 contracts from wire
types. `SchemaQuery` and its catalog belong to `capabilities/schema.rs`; generation
and retrieval never read the workspace. `protocol::Command` supplies shared command
identity, parameter metadata, and write policy. Requests map exhaustively to that
identity; schema generation binds request variants and concrete result types.
Discovery references generated contracts by content-derived identifiers. The
catalog is initialized once per process and contains bundled local definitions.
Serialization and deserialization schemas use separate contracts. Custom mutation
state and module-path schemas preserve their actual wire representation. The CLI
enables this through its default `schemas` feature; core and engine consumers opt
in independently, with no parser or transport dependency added.

`ReferencesQuery::definitions` remains available to typed callers as a separate
reference-evidence helper. The TUI definition pane uses `NavigationQuery` and does
not infer preview targets from unique names in its search results.

`Engine::run` returns an in-process `Execution`: `Completed(Answer)`,
`Preview(Planned<MutationAnswer>)`, or `Applied(Applied<MutationAnswer>)`. Only
mutation previews retain executable plans. `into_preview` and `into_applied`
reject a mismatched kind with a structured error; `into_answer` consumes the handle
at a reporting or wire boundary. `Execution` is not serialized. The CLI and serve
convert it to the wire `Answer`; the picker retains previews and applied
completions until it has extracted what its view needs.

A mutation answers with `Planned<T>`: immutable presentation data beside plans
and the captured history intent. `Apply` requires the sealed `Mutation` capability,
writes the plans, records history, and returns `Applied<T>` with a required history
id. Queries cannot carry `Planned` or reach `Apply`. Typed plans and completions can
widen to the closed `MutationAnswer` sum without losing their handles; they become
`Answer` only at the reporting boundary. `Planned` exposes its result through
immutable access and retains its captured intent independently of presentation.
Mutation payloads own a `MutationState`: `Preview` or `Applied { history_id }`,
serialized as the wire `applied` and `history_id` fields. Contradictory states
are rejected during deserialization.

`Intent` is a mutation description recorded by history and composed by batch.
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

Query types are available at both crate-root and `protocol::` public paths,
while the six mutation types above are root-only. This public-path inconsistency
is tracked in [backlog.md](backlog.md#api-path-unification); Rust import paths are
independent of the serialized request and answer contract.

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
at all. Repeated requests within the trust interval reuse the walk. The engine's
own writes (`Apply`, `Ledger::undo`) or `Engine::touched()` (the TUI after an editor
hand-off) expire that interval; the next graph access checks file stamps and reads
only changed contents.
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
unseeded cycles stay unresolved. Scope consumes the fragment's completed
resolutions. Move planners consume these edges,
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

Capability execution uses these components:

- `SearchQuery` asks `Graph::search_scoped` to filter candidates by paths, packages,
  and literals, then execute `Candidate::find` in parallel. Addressed declarations
  precede other matches.
- `RewriteIntent` narrows search matches and expands `Template` against each
  matched source snapshot. `RewriteOf` validates retained matches and captures.
- `RenameIntent` contains a `ReferencesQuery`. `Target` selects an addressable
  declaration per language, and `Scope` judges occurrences. Ambiguous declarations
  require `declared_in`; confidence and `Selection` determine the edits.
- `MoveIntent` builds a `MoveSet`, including layout-defined companions such as
  Rust's `a.rs` and `a/`. `Rebase` transforms resolved fragment edges;
  `Reachability` checks visibility and `Widen` supplies allowed modifier edits.
  Surgery supplies relocation edits from witnessed source snapshots.
- `MoveSymbolIntent` builds an `Extraction` and `SymbolMove` from the source,
  destination, and consumers. `Site::partition` assigns moving and staying edges
  before rebasing. Import provisioning, destination cleanup, and visibility edits
  contribute to one `Change`; validated extraction edits determine the cut and
  insertion. Rust `impl` pieces move with their type.
- `BatchIntent` plans and applies intents in order on a staged `Overlay`.
  `Receipt::then` composes their before-states; the preview compares the real
  before-state with the final staged contents and locations.
- `SurfaceQuery` lists public declarations and re-export aliases with importer
  counts. `ImpactQuery` follows importing modules breadth first.
  `DeadQuery` counts resolved references and unverified tokens per declaration.
- `ImportsQuery` classifies unresolved, repeated, and unused bindings from
  fragments. Re-exports and imports with hidden binding names are excluded from
  unused checks; files without a module address are reported as unplaced.
- `FileQuery` loads source text and asks the claiming language for facts, returning
  highlights and declarations from that same snapshot.
- `Ledger::history` reads validated entries. `Ledger::undo` checks receipt
  fingerprints, restores files, cleans owned directories, and saves the ledger
  through one transaction.

Mutation producers merge witnessed `Change` values. `Planned::of` binds the
result to a `ChangeSet`, rejects edit conflicts, previews it, and retains plans
and intent beside the typed result. `Apply` commits those retained plans and their
history entry through one `Transaction`.

The engine's supporting modules:

| Module      | Responsibility                                                                                  |
| ----------- | ----------------------------------------------------------------------------------------------- |
| `vfs`       | Filesystem operations and stamps; disk, memory, and staged overlay backends                     |
| `workspace` | Root and Vfs ownership, immutable `SourceFile` snapshots, source witnesses, and path conversion |
| `change`    | Witnessed edits and moves, notices, respellings, and conflict-checked binding                   |
| `plan`      | Preconditions, preview, transactions, receipts, immutable planned results, and fingerprints     |
| `protocol`  | Shared wire data, `Request`/`Answer`, envelopes, errors, styled lines, and vocabulary           |

Capability-specific wire types live with their execution and composition, as
listed in the command ownership index. [protocol.md](protocol.md) specifies the
serialized contract.

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
cross-volume copy flags. Occupied destinations, including distinct hard links,
are refused before invoking the native move. After native success, the source
must be absent: a successful no-op against a racing hard link is an unchanged
failure. The native operation still enforces destination preservation after the
initial check. Same-volume local renames are a single native operation;
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

Receipts also retain the directories actually created by the file plans, in
creation order; `Receipt::then` keeps that ownership across batch steps. Ledger-only
parent directories are excluded from the receipt. Undo removes owned directories
in reverse creation order only when empty. Pre-existing directories and directories
containing other files are retained. Receipts without directory ownership
evidence retain their directories. A removed directory has a
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
composition live beside their execution, requests, and results. Their types keep
crate-root and `protocol::` exports. File preview has no rendered document.

The shared report modules are `document.rs` (construction, shared helpers, and
`of`/`error` delegation), `block.rs` (structured blocks, notes, and reference plan
data), `row.rs` (rows and source sites), `view.rs` (View, Options, Presentation, and
Detailed), and `lines.rs` (shared line builders). `report/mod.rs` re-exports the
public vocabulary; capability-specific summaries stay with composition.

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
its `Request` on a session engine and writes the `Reply` for JSON-lines clients. Dependencies: `vvv-engine` and, behind the `tui` feature (on by
default), `vvv-tui`; `vvv ui` or bare `vvv` in a terminal hands the engine to the
picker with the CLI's colour policy and `$VISUAL`/`$EDITOR`.

The optional `mcp` feature adds `mcp/`, a six-tool read-only adapter using the official
Rust SDK's codec, lifecycle, and dispatch. It calls the engine in process through
`Call::execute_with_cancellation`, sharing the ordinary call budget policy. A bounded
queue and one dedicated engine worker keep synchronous parsing off the protocol
loop; the transport bounds frames, request IDs, and admitted requests. Tool descriptions
and result/error conversion live in `output/mcp.rs`. SDK and async-runtime dependencies
remain optional CLI dependencies, with no engine/core dependency changes.

`ReadCancellation` is a single-use engine read-call handle. Checks between reads,
search batches, navigation lookups, and context phases cooperate with cancellation.
The final publication gate serializes cancellation against query-store publication:
cancellation cannot publish a checkpoint; a completed publication ignores late
cancellation. Existing immutable checkpoints remain retryable. A parser invocation or
filesystem operation is not preempted. Closing MCP input cancels work and joins the
worker before releasing the engine and its handles.

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
- `model.rs` — application state and its transitions: the retained `Search` hub,
  current `Mode`, optional overlay, status, report-view choice, and split size.
  Common cursor, panel-focus, and file-preview types keep their methods here.
  `Model::mode_screen()`/`overlay_screen()` select screen metadata;
  `Model::focus()` returns the focused panel's index.
  `Model::action_for(event)` resolves through the global, focused-panel, screen,
  panel-default, and navigation key layers. `Model::update` routes application
  actions to the selected mode; `Model::on_event` checks generations before
  delivering answers. Both are pure and return effects. Entering a mode sends its
  first plan without debounce and sets `arriving`; `Model::shown` retains the
  search screen until that plan answers.
- `modes/<name>/mod.rs` — each mode's state and methods, including action and event
  transitions (`rename`, `moves`, `rewrite`, `history`, `search`). Its `screen.rs`
  defines a separate view type with its rendering methods. Search's `query.rs`
  owns the query bar; its state includes read-only references, impact, definition,
  and dependency relations. Mode inputs drive plans; queries drive searches, with
  generations preventing stale answers from replacing retained data.
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
  `screen/defaults.rs` holds the shared key layers. Each mode binds its view once;
  the application chooses the shown mode before panel rendering. Panels never
  receive `Model` or inspect `Mode`.
- `overlays/mod.rs` — menu, confirmation, help, and report state with selection,
  transition, and view-binding methods. `screen.rs` owns metadata and typed box
  views, each with its rendering methods. `Overlay` owns report-source selection
  and help scrolling. Help retains static screen metadata independently of a
  renderer.
- `modes/context.rs` — shared status and generation borrowed by a mode transition,
  without access to `Model` or another mode. `input.rs` holds `TextInput`, which
  edits a borrowed string buffer for name, destination, and template inputs.
- `render/` — the drawing primitives the screens compose. `Painter` owns the
  palette `Theme` and answers the drawing questions with one receiver (`caret`,
  `site`, `hit`, `line`, `source_window`); the colour policy is reachable through
  the painter (`Theme::role` maps a `display::Role`, `Theme::mark` colours a
  `Mark`). The stateful boxes are `Pane` (a bordered list/text box) and `Header`
  (the title card); a screen's `layout` splits a `Region` into panel regions
  (`columns`, `split`, `rows`), and `Fit` is a text helper. Words come from
  `protocol::vocabulary`.
- `worker.rs` — the engine on its own thread; effects in, events out; bursts of
  searches or plans are coalesced. Interleaved context/definition preview bursts
  keep the newest request of each kind; explicit queries and mutations are barriers.
  Definition successes and failures carry a ticket and query, checked against the
  search mode's current selection before settling the pane. `Commit` plans and applies in one step.
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

The search mode owns explicit browsing pages and a bounded navigation trail. Saved
results and file previews share immutable allocations; entries retain query,
selection, focus and scroll, never mutation plans. Explicit follows use a separate
request ticket from coalesced row previews and only successful follows push
history. Restoring a page validates its versioned occurrence and displayed target
before following again. Identifier/candidate choices belong to a typed navigation
overlay; the engine supplies source anchors, including identifiers in file previews.

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
`fingerprint.rs` own immutable mutation handles and content fingerprints,
respectively. Internal re-exports expose these components within the lifecycle.

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
are retained. These guarantees cover returned errors, not panics, and exclude
crashes and concurrent-writer isolation. They do not promise to restore inode
identity, timestamps, or complete metadata.

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

**Move destinations are preconditions.** A plan retains an absence requirement
for each destination, or a same-entry requirement for a case-only rename. Preview
and apply check every destination before any file write; an occupied destination
returns `exists`. Preflight does not reserve a path. Forward and recovery moves
also use `Vfs::move_if_absent`, which preserves a destination created after that
check. Case-only moves use two destination-preserving legs through a unique
name, with stored spelling retained for recovery and undo. The native operations,
fallbacks, and partial outcomes are specified in the lifecycle section above.

**Errors and notices are variants, not sentences.** `ResolveError` has one variant per
situation a layout can refuse (`Root`, `IntoItself`, `CrossProject`, `NoParentFile`
with the candidate files, …) and `Notice` carries a `NoticeKind`. `thiserror` gives
errors a canonical message. `EngineError::code()` maps failures to stable wire
codes; conversion to `Failure` includes a display message, an optional string hint,
and structured recovery details when restoration cannot be verified. Hints can
contain CLI syntax; structured hint actions and distinct Layout/Surgery errors are
open contracts tracked in [backlog.md](backlog.md).

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

## Retained query checkpoints

`query_store.rs` owns immutable typed checkpoints shared by clones of an engine.
Search and context execution remains with their capability owners;
`capabilities/pagination.rs` owns shared delivery budgets and continuation dispatch,
and `capabilities/excerpts.rs` owns exact source expansion. Query and excerpt tokens
reference one query root and expire or are evicted together. Failed publication
never consumes an existing checkpoint. Fixed monotonic lifetimes and conservative
byte accounting bound retained roots and checkpoint growth.

`graph/query_snapshot.rs` captures the full claimed input universe with content
identities, manifests, inventory, and walk configuration. Paged calls build fresh
graphs and revalidate before publication; graph stamp/trust caches cannot prove
cursor validity. Excerpt checkpoints retain a target and byte offset, loading its
source only through a validated fresh graph. No full workspace graph or source
body is retained by the query store. Search collection checks retained-result
limits incrementally; parsing and temporary graph allocations remain outside that
limit.

Operation exclusion precedes store access. Store locks are released before graph
construction, traversal, and validation; publication acquires the store only after
those succeed. `Engine::touched` increments a shared revision without taking a
store or graph lock, invalidating queries even after failed mutation attempts.
Engine dispatch routes typed requests without owning capability behavior.
