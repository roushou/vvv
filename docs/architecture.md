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
| `navigation` | `LexicalBinding`, `BindingRule`, `BindingNamespace`, `NamedImport`, `NamedImportRule`, `ModuleScope`, `ModuleDeclaration`, `ModuleScopeRule`: navigation facts and declarative extraction rules                                                                   |
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
  shares: an id, extensions, a `Grammar` and its `Semantics`, an explicit
  `NavigationSyntax` selection, and optionally a `Layout` and a `Surgery`.
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

The [typed syntax view design](typed-syntax-design.md) describes borrowed structural
views, declarative field accessors, and the separation from fact interpretation.
Rust syntax views own conditionals, ordered conditions, match arms, loops, let declarations,
closures, macro placement and uncertainty, constructor shapes, local import
coverage, inline-module owners, and call classification. `Impl`, `Trait`, `Struct`,
`Enum`, and `Type` inspect their declaration headers and share lexical binding
lowering through `HeaderBindings`; `Type` currently owns type aliases and their generic
scope, without type inference. TypeScript/TSX share structural callable questions
and a typed binding adapter that preserves grammar-owned scope and coverage rules.
Generic and TypeScript pattern views share the `PatternNames` rule policy for
simple/custom captures. TypeScript callable/catch headers, lexical blocks, and typed loop headers use `PatternBindings`
for atomic destructuring and initializer/default/computed-key evaluation order.
`DeclarationScope` owns file, block, and switch declaration environments; `VarBindings`
collects declarations for callable, file, static, and namespace owners. Strict-scope
evidence controls block functions, while `CallableScope` retains parameter/body
environment compatibility. Assignment patterns publish reference roles without bindings.
Conflicts are validated after all owners publish so traversal order cannot hide them.
Parameter and block bindings retain initialization regions separately from lexical ownership;
the engine checks initialization after selecting the innermost owner. Rust
retains its richer pattern-role and alternative-binding interpretation. Rust imports and modules share typed grouped-import, alias, body, and
visibility accessors; TypeScript/TSX source statements and specifiers retain their
distinct forms. Grammar rules still own capture coverage and mutation scope;
generic table extraction remains available to other grammar-backed
users. A shared declaration component bridges symbol and signature rules to
language-specific header views and supplies companion target/shadowing inspection.
Callable initializer signature rules retain direct variable/field headers and
navigation-only expression-name headers without adding symbols or mutation coverage. Parser views are temporary and never cross the plugin boundary.

Small language test groups live beside the code they exercise. Larger Rust suites
live in `vvv-lang/src/rust/tests/`, grouped by capability and included as test-only
modules by `rust/mod.rs`. Language tests check extracted facts, source spans, scope
boundaries, layout, and edit spelling independently of command output. The command
corpus in `vvv/tests/corpus/` checks the composed behavior with real language plugins
and exact human and JSON output.

### `vvv-engine` — the façade

`Engine::new(workspace, languages)` — `Workspace::disk(root)` for the binary and the
registry from its composition root (`vvv-rs`'s `languages.rs`), a `MemoryVfs` and a fake
language for tests; `with_retention`, and `with_oracle(Arc<dyn Oracle>)` for a host that has a
build or a language server to ask — and one wire dispatcher, `Engine::run(Request)`.
The dispatcher holds a shared operation guard and delegates to capability-owned
execution bodies. It acquires the graph only for source-tree questions and
planning. History, undo, retained-plan apply, and file preview do not refresh the
tree. The engine owns orchestration, not capability behavior.

`WorkspaceFilesQuery` returns the fresh visible workspace inventory as sorted,
deduplicated `RelPath` values. It shares the file capability, uses the workspace
walk without a language restriction, and needs no graph or parser. Workspace
browsing in the TUI filters this data locally and uses `FileQuery` for text and
outline declarations from one source snapshot.

The file capability retains up to 32 parsed previews within a conservative 64 MiB
payload budget, shared by engine clones. Every file request first reads current
contents and checks their complete digest; graph trust, timestamps, and explicit
invalidation are never substitutes for that read. Changed contents replace the
cached version, least-recently-used files are evicted, and oversized previews are
returned without retention. The cache stores successful plain file results only,
with no parser nodes, navigation decisions, or errors.

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
boundaries, unsupported binding forms, and exact named-import rules.
`NavigationSyntax::Tables` retains the generic table path; Rust selects
construct-owned extraction in `syntax/rust/`; TypeScript/TSX select structural
callable views and header-owned parameter interpretation in `syntax/typescript.rs`,
with table fallback for other binding rules. `Block`, `Function`, `Pattern`,
`LetDeclaration`, `Conditional`, `Match`/`MatchArm`, `Loop`, and `Closure` retain
parser views only during extraction.
Their types and behavior remain together; parser nodes never enter shared facts.
Parse-local `NavigationCoverage` holds modeled and blocked spans plus cached owner
exclusions, and both identifier passes consult it. The syntax adapter lowers
these to plain `Facts`: eligible token spans, `LexicalBinding`s,
`NamedImport`s, `ModuleScope`s, `ImportScope`s, and export restrictions. Block-wide
named import ownership is navigation-only; grouped prefixes do not introduce
bindings. Resolution uses the import's own block and enclosing import scopes,
never an inner block at the use site. Import targets establish namespace and
constant-pattern evidence before comparison with lexical bindings. Rust module facts record
direct declarations, scoped imports, and visibility restrictions independently of
the file-level facts used by mutation planners. `ModuleNavigation` resolves these
scopes against the language layout, retains competing targets, and captures every
consulted source. Import traversal has cycle guards, a 1,024-step budget, and
128-level recursion/provenance bounds. Navigation checks the innermost visible
binding before module lookup, keeps type and value namespaces separate, and uses
exact named bindings for TypeScript. Navigation-only declarations stay separate
from ordinary search/mutation symbols. `ScopeUncertainty` records block-owned macro invocations separately from token
eligibility. The syntax adapter distinguishes grammar-declared required-expression
positions; unknown positions retain uncertainty. `NavigationScope` checks known
binding precedence before lexical resolution and blocks unrooted module lookup
where expansion could introduce competing items. Binding markers distinguish
explicit mutable bindings from identifier patterns that could name generated
constants. Unknown generated items remain
relevant in nested functions; no macro expansion or name whitelist is assumed.
These facts do not affect mutation planners. Unsupported patterns and scope forms block
confirmation instead of falling through to a same-named outer declaration.

Rust struct patterns separate constructor/field labels from shorthand and renamed
bindings, support rest fields and per-name ref/mut evidence, and reject unsupported
nested fields without publishing a prefix. Explicit labels are excluded from lexical
navigation. Rust tuple-struct patterns such as `Some(value)` extract their supported inner
bindings without treating the constructor as a local binding. In `let … else`,
those bindings become visible after the complete declaration; its initializer and
`else` body retain the outer bindings. Nested unsupported patterns retain the
conservative scope barrier. Constructor resolution still requires independent
navigation evidence; pattern extraction does not infer types or expand macros.

Rust condition bindings use a synthetic owner from the start of the conditional
through its consequence, excluding the alternative. Each binding starts after its
own condition operand, so later operands and the consequence can use it; the
initializer, alternative, and following statements retain outer bindings. Smaller
consequence block scopes preserve local/import precedence. Unsupported condition
patterns block the conditional without poisoning surrounding statements. Module
constant/import evidence is consulted before confirming an immutable pattern as a
new variable; unavailable constructor evidence remains conservative. See
[syntax-navigation-design.md](syntax-navigation-design.md) for construct ownership
and extraction invariants.

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
incoming scans inspect a bounded number of files and confirm original-name and
named-import-alias uses
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
and the thin MCP `vvv_relationships` tool. `capabilities/incoming.rs` owns per-file named-alias discovery shared by one-shot
context, context pages, and relationship pages. It retains import probe progress,
resolved local spellings, site spans, and the next site position. Each use still
resolves independently; a spelling alone never confirms an edge. Plugins declare
`Language::reference_group`; TypeScript and TSX share a group without language names
in the engine. Relationship checkpoints retain file position, alias discovery, and
a pending undelivered site, and use the existing immutable cursor store. No complete
source or graph is retained, and byte fitting never drops a pending site. No
relationship state or source inference lives in an interface, and this query does
not change mutation resolution.

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

A file's **`Fragment`** (`graph/fragment.rs`) holds its structural edges as data:
its module address, every addressable declaration placed (`Declared`: symbol,
address, reach), and import statements and qualified paths resolved (`Edge`).
`ImportBindings` follows same-file bindings and visible module-level bindings in
other files. Language facts identify module bindings and their visibility without
exposing parser nodes. Lookup retains every distinct target, binding provenance,
and consulted source versions; unseeded cycles stay unresolved. Recursive lookup
checks cancellation and limits depth. Each path prefix is resolved separately,
because an alias can change the shape of its address.

The graph shares `SourceFacts` between candidates and namespace snapshots. These
hold source contents and lazy parser facts, without fragment or scope caches, so
cross-file lookup cannot create recursive cache locks or ownership cycles.
Scope consumes the fragment's completed resolutions. Move planners consume the
same edges, including for import provisioning and destination cleanup. `Rebase`
transforms resolved addresses and preserves an alias spelling when rebasing its
binding already supplies the required target; it does not resolve raw paths again.
A candidate reuses its fragment while its project identity and every consulted
source version and consulted module inventory match, including previously missing
providers. The retained `Arc<Project>` identifies the project build;
a changed project invalidates every fragment and scope. A scope is reused only
with the same fragment. Navigation captures binding-provider sources in its
snapshot and revalidates them before delivering an answer.
A file's **`Scope`** — what it
sees: bound names, opened modules, resolved and unresolved paths — is read off the
fragment and kept beside it, so `references` judges tokens per name through a lookup;
tokens in the middle of a path use the fragment's captured prefix resolutions. `aliases_of`
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

The optional `mcp` feature adds `mcp/`, a seventeen-tool adapter for navigation, reviewed mutations, and explicit validation using the official
Rust SDK's codec, lifecycle, and dispatch. Reads and validation call the engine
in process through `Call::execute_with_cancellation`; apply uses `Call::execute`
and finishes its active transaction. Both share the ordinary call budget policy. A bounded
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
  `Model::action_for(event)` resolves through the global, recovery, input-editing, focused-panel, shared preview-inspection, screen,
  panel-default, and navigation key layers. `Model::update` routes application
  actions to the selected mode; `Model::on_event` checks generations before
  delivering answers. Both are pure and return effects. Entering a mode sends its
  first plan without debounce and sets `arriving`; `Model::shown` retains the
  search screen until that plan answers.
- `modes/<name>/mod.rs` — each mode's state and methods, including action and event
  transitions (`rename`, `moves`, `rewrite`, `history`, `search`). Workspace browsing
  owns its state and typed screen in `modes/workspace/`, retained as a search browsing
  page so the existing trail restores both workspace and search contexts. Workspace
  state owns independent file/outline viewports, local outline filtering, per-file
  inspection positions, and revision-checked pointer selection. Its typed view owns
  the same wrapped-row geometry used by rendering and hit testing, including narrow
  layouts. Its `screen.rs` defines a separate view type with its rendering methods. Search's `query.rs`
  owns the query bar; `files.rs` owns fuzzy file ranking, filter edits, file and match viewports, remembered
  occurrence selections, and stable pointer targets; `locations.rs` keeps result scope and directory suggestions
  separate from global navigation. Its state includes role categories, read-only
  references, impact, definition, and dependency relations. Mode inputs drive plans; queries drive searches, with
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
  set of bindings with `resolve`. `Key::from_event` in `keys.rs`
  turns a crossterm event into a `Key`, folding shift into the character.
- `screen/` — shared key, focus, and help metadata (`Screen`, `Panel`) and
  the application frame. Help and footer rows resolve each binding against actual
  precedence and current conditions, suppressing shadowed aliases. `BoundScreen<V>` owns a typed view and its layout and panel
  callbacks; panels render from that view without inspecting `Mode`.
  `screen/defaults.rs` holds the shared key layers. Each mode binds its view once;
  the application chooses the shown mode before panel rendering. Panels never
  receive `Model` or inspect `Mode`.
  The shared bottom bar is the sole passive key legend, derived from effective
  bindings for the focused pane or overlay. Borders hold content metadata.
  Overlays render within the body, leaving the bottom bar visible.
- `overlays/mod.rs` — menu, confirmation, help, and report state with selection,
  transition, and view-binding methods. `screen.rs` owns metadata and typed box
  views, each with its rendering methods. `Overlay` owns report-source selection
  and help scrolling. Help captures effective binding rows at its originating focus and retains an
  underlying overlay for cancellation. `places.rs` owns the searchable trail and
  recent-recipe picker.
- `modes/context.rs` — shared status and generation borrowed by a mode transition,
  without access to `Model` or another mode. `input.rs` holds `TextInput`, which
  edits a borrowed string buffer and its owning input’s `Caret` across queries,
  filters, pickers, inspection, and mutation inputs. Grapheme and word motion are
  pure and do not trigger effects; a paste is one edit.
- `problem.rs` — structured engine failures, retained retry effects, recovery paths,
  and problem-pane scrolling. Mode transitions rebuild previews rather than
  repeating failed writes. The footer derives available recovery actions from
  these facts.
- `render/` — the drawing primitives the screens compose. `Painter` owns the
  palette `Theme` and answers the drawing questions with one receiver (`caret`,
  `site`, `hit`, `line`, `source_window`); the colour policy is reachable through
  the painter (`Theme::role` maps a `display::Role`, `Theme::mark` colours a
  `Mark`). The stateful boxes are `Pane` (a bordered list/text box) and `Header`
  (the title card); a screen's `layout` splits a `Region` into panel regions
  (`columns`, `split`, `review_rows`), and `Fit` is a text helper. Words come from
  `protocol::vocabulary`.
- `worker.rs` — the engine on its own thread; effects in, events out; bursts of
  searches or plans are coalesced. Interleaved context/definition preview bursts
  keep the newest request of each kind; explicit queries and mutations are barriers.
  The inbox is drained again between operations so a slow read cannot force
  already superseded previews from its original batch to execute.
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

Operation modes share a typed `ReviewState` for border metadata and apply hints.
Rename and rewrite preview the exact checked `MatchId` selection; each input or
selection change advances its generation, and apply waits for the corresponding
preview. Empty inputs and selections invalidate pending previews without engine I/O once
selection is available. Rename completes its first judgment with the engine's
default selection before relying on checked IDs. Applying modes reject input and selection edits until completion. File-grouped review
rows are a shared rendering component: complete paths stay in reading order, source
excerpts retain hit styling, and display offsets map back to the mode's item cursor.
Compact sibling panes leave remaining space for the retained active list; short
layouts collapse siblings to borders. Detail focus preserves the last list and
text boundaries scroll content rather than changing the selected occurrence.

Workspace UI preferences are pure, versioned data in `preferences.rs`.
`tui.rs` owns their bounded loading and saving outside the project tree, keyed by
canonical workspace; restoring them sets layout choices and recent recipes,
never the active query or restrictions. Search's `recall.rs` owns bounded,
validated query/location/category/file-filter recipes, separate from source pages.
Forgotten recipes remain suppressed until explicitly searched again in the session.

The search mode owns explicit browsing pages and a bounded navigation trail. Saved
results and file previews share immutable allocations; entries retain query,
result location, category, file filter, per-file selection, focus, preview scroll
positions, inspection state and preview expansion, never mutation plans. Search scopes constrain result files; definition navigation
continues to use the whole workspace. File and match lists share the left column;
Files and Matches have independent focus and viewports. The selected file's
occurrences stay within their file during match navigation. Search view geometry
supplies both rendering and pointer hit testing; the terminal loop maps mouse input
against the last presented frame, and pure transitions validate stable identities
and result revisions before accepting selection. Wheel scrolling preserves focus
and selection. The terminal session owns mouse capture and suspends it for the editor. Local fuzzy filtering ranks retained file groups and preserves
stable match identities without changing query execution or mutation selection.
Reference files group all visible confidences together; verdicts remain engine data.
Search restrictions share one typed vocabulary for border metadata and the filter
menu. Pickers count retained search matches, before category and fuzzy file filters;
counts are never estimates of unsearched workspace facets. Pending or stale searches
omit them. Filter navigation retains a hidden match identity until explicit result
navigation or query editing replaces it. Menu text clearing and restriction clearing
are separate pure actions; resetting restrictions preserves the query’s pattern and
name. Definition resolution remains independent of result location.
Each result page memoizes file grouping and occurrence order by immutable answer
identity, category, relation, and location. Fuzzy edits rerank those file groups
without regrouping occurrences. Cursor movement and views reuse the projection;
its offsets refer to retained answers rather than copying matches, and its payload
is charged to the navigation trail's bounds. Rendering styles only visible list
rows while pointer geometry retains stable identities for the full viewport.
Source previews index syntax ranges for visible-window lookup while preserving
the original priority of overlapping and multiline highlights.
Source preview anchors retain the displayed file's location and hit while a new
file loads; its text, anchor, and scroll position change together on an accepted
preview. Pending file selection never relabels retained text as the destination.
Refresh, editor return, and newly observed match content identities mark the source
for replacement even when the selected path stays the same.
Each preview owns an inspection state keyed by displayed path, content identity and
source range. Literal find spans use source byte offsets; next-hit navigation,
line validation and horizontal scrolling are pure transitions over retained bytes.
Source inspection covers the file; definition inspection covers its displayed
enclosing declaration. Rendering clips syntax and find highlights in terminal
columns, expands tabs and preserves fixed gutters. Find drafts retain their prior
position for cancellation. Explicit find and line jumps supply editor sites;
ordinary preview scrolling preserves the selected occurrence's editor site.
Expansion retains hidden list viewports and follows explicit preview focus; leaving
preview focus restores the split. Inspection allocations count toward navigation
history's retention bounds.
Wide previews reserve fixed source and definition regions independent of selection,
loading, and empty results. Single-preview layouts reveal source or definition
according to focus and the retained preview choice; selecting a result does not
change that choice. Explicit follows use a separate
request ticket from coalesced row previews and only successful follows push
history. Completed query and filter changes retain the departing page without
recording pending keystrokes. A direct trail jump moves the entire route before
applying retention bounds and validates only the destination. Restoring a page validates its versioned occurrence and displayed target
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

**Symbol moves select declarations and retain ownership evidence.**
`capabilities/moves/selection.rs` owns candidate enumeration, identity, and explicit
single selection. All matching addressable declarations remain visible, with typed
unsupported reasons. `SymbolRule::companion_of` describes companion target kinds;
`syntax/symbols.rs` emits `Facts::declaration_pieces` with source spans, top-level
scope and move-support evidence, and typed companion ownership. No parser node crosses the plugin
boundary. Same-scope unqualified targets establish ownership; competing declarations,
qualified or shadowed targets, and cross-scope targets without a local owner do not.
Module declarations, nested declarations, and competing module/name bindings are refused by the planner.
The engine's existing module/name addresses cannot identify conditional/overloaded
peers separately. Extraction receives the selected declaration and explicit owned
pieces, never a name-based collection. Retained symbol moves use this same planner.

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
   an `impl X { const ID; }`, and an `impl Default for X` calling `AstGrepLanguage::new(ID,
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
Search, context, and relationship execution remains with their capability owners;
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

## Retained mutation handles

`capabilities/plans.rs` owns preparation, inspection, application, discard, and
report composition for reviewed rename, rewrite, file/directory move, and selected
symbol move plans. `plan_store.rs` retains executable plans and terminal outcomes, shared by engine clones. Ordinary mutation requests
keep their existing lifecycle. Session handles are references to captured plans;
clients cannot submit replacement edits or change the captured intent on apply.

Preparation uses a fresh content-verified `QuerySnapshot`, preserves any configured
library oracle, plans against that graph, and revalidates before publication. It
checks the complete review or requested first page against its output budget before retaining a handle. Application
consumes the retained executable plan after operation exclusion, revalidates the
input snapshot and engine revision, and delegates writes/history/recovery to `Apply`.
This session capability scans before apply; direct typed `Apply` remains a
fingerprint-bound operation without refreshing the source graph. Store locks never
span graph construction or filesystem work.

Successful receipts are retained so retries cannot write twice. Failed attempts
retain their structured failure and cannot execute again. Fixed monotonic expiry,
entry limits, and conservative byte accounting bound the store; capacity pressure
never evicts pending plans or successful receipts. Discarded tombstones may be
reclaimed on the next preparation. MCP distinguishes queued cancellation from
active application: reads are cooperative, active writes finish their transaction.

`capabilities/plans/review.rs` owns the immutable closed rename/rewrite/file-move/symbol-move preview,
record construction, exact UTF-8 text chunking, delivery budgets, and review cursor
positions. `PendingPlan` holds `Planned<MutationAnswer>`; preparation admits only
rename, rewrite, file/directory moves, and selected symbol moves. Shared immutable
preview allocations keep inspection from copying all occurrences and edits. Review pages never access a source tree or
reuse source-query checkpoints: they describe captured evidence that remains
readable after workspace edits, apply, or failed apply. Lifecycle and latest
validation evidence remain inspection's responsibility.

Stateless review cursors identify checked positions within a plan's captured records;
page retries add no retained checkpoint allocations. Captured reviews remain charged
within the existing plan memory limits after completion, and share the original
fixed expiry. Discard releases pending executable plans, captured reviews, and
baselines. No proof of page delivery is required for apply; clients own their review
policy. Batches are not admitted to retained handles.

### Validation of applied plans

`capabilities/validation.rs` owns explicit check commands, budgets, version evidence,
execution, and report composition. Its `process.rs` owns shared polling and bounded, cancellable pipe capture;
platform backends own Unix process groups or Windows Job Objects. Unix keeps the
leader unreaped until group cleanup to prevent group-ID reuse. On macOS, a group
signal can return `EPERM` when only its zombie leader remains; safe `libproc`
enumeration must confirm the exited leader is the sole group member before treating
that result as completed cleanup. Enumeration failures and denials involving other
members remain errors. The Windows backend uses safe synchronous dependency APIs to assign suspended children before
resuming them and to capture nonblocking byte pipes. It terminates descendants
after leader exit, timeout, or cancellation, and closes parent writer handles
before waiting for EOF. Windows launches native executables only; invoking a
shell is an explicit caller choice. `Workspace::disk` enables execution; virtual and
staged workspaces cannot launch programs against their unrelated host paths.
The engine names no language or build system. The caller supplies program/argv,
which run with inherited permissions/environment and can have side effects outside
the transaction model. This external execution capability does not format or plan
source edits and provides no rollback or sandbox. MCP advertises it as non-read-only
and open-world. Engine-owned mutations still require a `Plan`.

`capabilities/validation/baseline.rs` derives expected post-apply input identities
from the executable preview before writes. It retains reviewed path transitions,
original inventory, exact final contents, and source/destination ancestor ignore
configuration with absence evidence. After successful apply, the store retains this
baseline rather than capturing and trusting a fresh tree. Move validation compares
inventory outside the reviewed transitions, verifies old-entry absence (or case-only
aliases), and checks final destination contents directly even when hidden or ignored.
Move observations also track entry spelling and configuration presence during checks.
`QuerySnapshot` remains the strict pre-apply and query freshness contract; an overlay
walk is not evidence of disk ignore behavior at destination paths. Validation first
checks this baseline, then fingerprints visible files, planner configuration inputs,
and caller-listed extra inputs before, between, and after commands. A report links
the apply receipt to these observations and bounded command results. It preserves
failures/cancellation independently of apply success. The latest report replaces
its predecessor within the plan's existing memory/expiry bounds. Capacity and
metadata budgets are checked before execution; serialization only trims logs.

Operation exclusion covers the batch, but store locks never cover process execution
or input capture. Cancellation can stop validation without cancelling an apply
transaction. Recording the report and final source observation ignore cancellation
so evidence survives an interrupted response; the client can inspect it. Checks
invalidate graph trust because external programs can create or change files.

Pattern navigation evidence crosses the plugin boundary as plain data: reference
roles, alternative binding sites/modes, constructor shapes, and enum ownership.
The parser retains tentative bindings; the engine classifies names and validates
alternative binding sets through the scoped module resolver. These facts remain
navigation-only and do not expand mutation addresses or import rewriting.
