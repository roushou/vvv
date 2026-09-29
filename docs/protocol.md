# JSON protocol

Pass `--json` to any command. Output is a single JSON document on stdout, including
errors, so a client never needs stderr. Exit code is `0` for `ok`, `1` for `error`.
Or run `vvv serve` and send the same commands as JSON, one per line (below).

The wire types are exported by `vvv-engine`. Shared types live in `protocol/`;
capability-specific requests and answers live with their implementations and are
exported at the crate root. See [architecture.md](architecture.md) for Rust import
paths. A Rust client needs only `vvv-engine`. This page specifies serialization. Field order is not significant. Absent optional
fields are omitted, not `null`.

## Envelope

```json
{ "status": "ok", "schema": 1, "result": { … } }
{ "status": "error", "schema": 1, "code": "no_such_symbol",
  "message": "no struct named `Point`", "hint": "declarations are matched by exact name; …" }
```

`schema` is the version of this contract. It is bumped when a field changes meaning or
goes away; adding a field does not bump it, so a client should ignore fields it does
not know. An error's `code` is stable and meant to branch on; `message` and `hint` are
for people (see [Errors](#errors)).

## Requests

Every command is also a value — what `vvv serve` reads, and what a Rust client hands
to `Engine::run` as `vvv_engine::Request`. The `command` field names it; the other
fields are the command's own, with the same names as the CLI's flags:

```json
{ "command": "search", "name": "Plan" }                      // pattern, kind, symbol, name, language
{ "command": "outline", "path": "src/plan/mod.rs" }
{ "command": "references", "name": "Plan", "declared_in": "src/plan/mod.rs" }
{ "command": "where", "name": "Plan", "from": "src/lib.rs" }
{ "command": "deps", "path": "src/plan/mod.rs" }
{ "command": "explain", "path": "src/plan/mod.rs", "position": { "line": 48, "column": 11 } }
{ "command": "surface", "package": "vvv_core" }
{ "command": "impact", "name": "Plan" }
{ "command": "dead", "language": "rust" }
{ "command": "imports", "path": "src/plan/mod.rs" }
{ "command": "file", "path": "src/plan/mod.rs" }
{ "command": "discover" }
{ "command": "context", "origin": { "kind": "position", "path": "src/engine.rs", "position": { "line": 8, "column": 15 } }, "budget": { "max_bytes": 8192 }, "references": false }
{ "command": "navigate", "origin": { "kind": "position", "path": "src/engine.rs", "position": { "line": 8, "column": 15 } } }
{ "command": "rename", "name": "Config", "to": "Settings", "apply": true }
{ "command": "rewrite", "query": { "pattern": "$A.unwrap()" }, "template": "$A?" }
{ "command": "move", "from": "src/a.rs", "to": "src/b.rs" }
{ "command": "move_symbol", "name": "Config", "from": "src/util.rs", "to": "src/config.rs" }
{ "command": "batch", "intents": [ Intent, … ], "apply": true }
{ "command": "history" }
{ "command": "undo" }
```

A mutation is its [intent](#intent) plus `apply`: absent or `false` previews, `true`
writes and records one undo — what the CLI's `--apply` does. So an `intent` object
printed by `--json` is a request as it stands. `position` is zero-based, like every
`Position`. The answer is the `result` the command prints under `--json`, exactly.

### `vvv serve`

A session over stdin and stdout: one request per line in, one reply per line out, in
order, until stdin closes. The tree is kept between requests and re-read only where it
changed. An optional `id` — any JSON value — is echoed on the reply, even when the line
could not be read:

```console
$ printf '%s\n' '{"id": 1, "command": "where", "name": "Plan"}' '{"id": 2, "command": "search"}' | vvv serve
{"id":1,"status":"ok","schema":1,"result":{"name":"Plan","sites":[…]}}
{"id":2,"status":"error","schema":1,"code":"bad_query","message":"a query needs at least one of: pattern, kind, symbol, name","hint":"…"}
```

Replies are compact (one line); a line that is not a request is answered with
`bad_request`. `vvv serve` runs the workspace given by `-C` for the whole session.

Calls optionally accept `max_output_bytes` (1,024 through 1,048,576). It counts the
compact UTF-8 JSON serialization of `result`, including escaped characters, but
excludes the envelope, echoed ID, and terminating newline. Without it, existing
commands retain their previous output behavior. Budgets apply to read-only commands;
mutation commands, including previews and undo, reject the option before execution.
They never write and then lose their receipt to an output-size error.

```json
{
  "id": 7,
  "command": "file",
  "path": "src/engine.rs",
  "max_output_bytes": 2048
}
```

An oversized result returns `code: "output_limit"` and
`output_limit: { "max_bytes": 2048, "required_bytes": 9012 }`. It is not a truncated
success. Increase the limit, narrow the request, or use bounded `context`. Error
envelopes are not subject to this result budget. For a context call, the effective
`budget.max_bytes` is the smaller of its requested/default budget and the call limit.
This limits output, not parsing, computation time, or in-memory result construction.

## `vvv discover`

`{ "command": "discover" }` returns the envelope schema version, registered
`languages`, `commands`, `context_defaults`, `context_maximum`, `min_output_bytes`,
and `max_output_bytes`. Each command entry has its `command` name, `read_only`
status (true only when every form is read-only), and accepted top-level `parameters`.
This is capability metadata, not a complete JSON Schema or a guarantee that every
construct of a registered language is resolvable. `id` and `max_output_bytes` belong
to the call envelope rather than individual command parameter lists.

## `vvv context`

The request accepts the same `origin` and `selection` as `navigate`, an optional
`budget`, and `references` (default false). Budget fields default independently:

| Field         | Default | Allowed range |
| ------------- | ------- | ------------- |
| `max_bytes`   | 16384   | 1024–1048576  |
| `max_items`   | 12      | 1–64          |
| `max_lookups` | 64      | 1–512         |
| `max_files`   | 64      | 1–1024        |

The result contains `snapshot`, `outcome`, `items`, `omissions`, and
`references_by_name`. Outcomes are `resolved`, `ambiguous` (with complete
`candidates: [{id, target}]`), or `unavailable` (with a navigation `reason`). An
ambiguous candidate set exceeding the byte limit returns `output_limit`; the
engine does not discard candidates or pick one to satisfy a budget.

Each item has:

- `target`: the complete versioned `SymbolRef`, including its full declaration range.
- `relation`: `definition`, `enclosing_declaration`, `referenced_definition`,
  `reference`, or `reference_in_test_path`.
- `via`: the exact occurrence establishing the relationship, or null for the seed.
- `excerpt`: a `SourceAnchor` for precisely the returned bytes.
- `start`: the excerpt's zero-based line/character position.
- `text`: the source substring, without added ellipses or reformatted indentation.
- `complete`: whether the entire target extent is present.

Items are ordered seed first, enclosing declaration second, outgoing occurrences
in source order, then incoming files/tokens in path/source order. Each target
appears once, keeping its first relationship evidence. Outgoing declarations
already contained in the seed are not duplicated. Documentation attached to a
declaration belongs to its extent; there is no separate generated summary.

`omissions` counts `item_limit`, `byte_limit`, `lookup_limit`, `file_limit`,
`ambiguous`, `unavailable`, and `no_container`. Counts describe items or occurrence
attempts as appropriate; they are not a total count of unseen relationships.
The seed lookup is outside `max_lookups`; that budget bounds subsequent resolution.
`max_files` bounds incoming reference scanning. Source output is bounded even when
an individual declaration is much larger than the budget; partial excerpts end at
UTF-8 boundaries and retain exact ranges. A result with omissions is useful partial
context, not evidence that no other dependencies or references exist.

Incoming scanning is opt-in and only considers the target's exact spelling, then
validates each occurrence through navigation. `reference_in_test_path` means a
confirmed reference with a `test` or `tests` path component; it does not claim that
the enclosing declaration is a test or that it covers the target's behavior.
No inferred caller/callee classification or alias-complete reference set is promised.

All consulted source/manifest versions from constituent lookups and scanned files
are revalidated before returning. The context snapshot incorporates those inputs
and constituent navigation snapshots. Observed changes return `stale`; this does
not lock external editors or promise filesystem transaction isolation. Expand an
item with a new context request using its `target` as a symbol origin.

## Shared types

**Span** — half-open byte range in the file.

```json
{ "start": 288, "end": 340 }
```

**Position** — zero-based line and _character_ column.

```json
{ "line": 12, "column": 4 }
```

**Symbol** — a declaration; present on a match when the matched node is one.

```json
{ "kind": "method", "name": "new",
  "name_span": Span, "span": Span, "extent": Span,
  "visibility": { "span": Span, "text": "pub(crate)" } }
```

`kind` is one of `function`, `method`, `struct`, `class`, `enum`, `variant`, `trait`,
`interface`, `type-alias`, `const`, `static`, `variable`, `field`, `module`, `macro`,
`impl`. `name_span` is the identifier, `span` the declaration node, `extent` the
declaration with what belongs to it in the text — leading attributes and doc comments,
an enclosing `export` — which is what moving it cuts. `visibility` is the modifier as
written; absent when there is none.

**Match**

```json
{
  "id": "21fcff22333d",
  "content": "<BLAKE3 digest of the complete source text>",
  "path": "crates/vvv-core/src/lang/registry.rs",
  "language": "rust",
  "span": { "start": 288, "end": 340 },
  "start": { "line": 12, "column": 4 },
  "end":   { "line": 14, "column": 5 },
  "kind": "function_item",
  "text": "pub fn new() -> Self {\n        Self::default()\n    }",
  "line": "    pub fn new() -> Self {",
  "captures": { "NAME": { "span": …, "text": "new" }, "ARGS": [] },
  "symbol": { "kind": "method", "name": "new", … },
  "role": "declaration",
  "address": { "package": "vvv_core", "path": ["lang", "registry", "new"] }
}
```

- `content` is the complete source version. Engine-produced matches always include
  it; older serialized matches without it still deserialize. It is independent of
  `id`, which retains its existing selection semantics.
- `path` is relative to the workspace root.
- `kind` is the tree-sitter node kind; `symbol` is the language-neutral view (above).
- `role` is where the match sits: `"declaration"` (it is the declared item — `symbol`
  is then present), `"import"` (inside an import statement), or `"use"` (anything
  else).
- `address` is present for a declaration in an addressable file: the same value
  `outline` gives it. Absent otherwise.
- `line` is the full source line on which the match starts (no terminator), so a
  client can show the hit in context without reading the file; `start.column` indexes
  into it by character.
- `captures`: `$X` yields one `{span, text}`; `$$$X` yields an array (possibly empty).
- `id` is derived from `path`, `span` and `text`. It is stable across runs while the
  file is unchanged, and different for any other match. `--select` accepts ids as well
  as the 1-based row numbers human output prints (positions in `matches`).

**Edit**

```json
{ "span": { "start": 10, "end": 19 }, "replacement": "baz(1, 2)" }
```

**FileChange**

```json
{ "path": "app.ts", "moved_to": "lib/app.ts", "edits": [ Edit, … ],
  "diff": "--- a/app.ts\n+++ b/lib/app.ts\n@@ …" }
```

`path` is the file before the change; `moved_to` is present only when the plan moves
it, and the diff headers then name both. `edits` are sorted by span and never overlap.
`diff` is a unified diff with 3 lines of context.

**Notice** — something a plan found but did not change, as data: a `kind` tag plus
the fields that kind needs. The plan is still valid; the notice is what remains to do
by hand.

```json
{
  "path": "src/net.rs",
  "start": { "line": 2, "column": 18 },
  "kind": "unrewritable_import",
  "import": "crate::util::parse::Config",
  "replacement": "crate::net::parse::Config"
}
```

Kinds: `unrewritable_import` (`import`, `replacement`); `redundant_import`
(`import`); `unreachable` (`item`,
`from`: the Address that references it after the move, `needs`: the reach it would
take — `everyone` when the consumer is in another package, which vvv never grants on
its own; the plan is still valid, the widening is yours to make).

## `file` (preview)

```json
{ "path": "src/error.rs", "text": "…",
  "highlights": [ { "span": Span, "kind": "keyword" }, … ],
  "symbols": [ Symbol, … ],
  "identifiers": [ SourceAnchor, … ] }
```

Text, syntax highlights, declarations, and identifier anchors come from the same
loaded source snapshot. `identifiers` includes every grammar-recognized identifier
in the file, with the path, complete-file `ContentId`, and absolute UTF-8 byte span
used by `navigate`. Arrays are omitted when empty; files without a language have
no syntax data. Older payloads without `identifiers` deserialize to an empty list.
Adding this field changes Rust `File` struct literals.
Clients can use declaration containment to preview an enum when a variant is selected.

## `navigate`

`NavigationQuery` contains `origin` and an optional `selection` using the existing
`Selection` shape (`"all"`, `{ "ordinals": [2] }`, or `{ "ids": ["…"] }`). An explicit
selection must keep exactly one candidate. The CLI accepts `--select ROW_OR_ID`.
Origins are tagged by `kind`:

```json
{ "kind": "position", "path": "src/app.rs",
  "position": { "line": 3, "column": 12 }, "expected_content": "…" }
{ "kind": "occurrence", "anchor": SourceAnchor }
{ "kind": "symbol", "symbol": SymbolRef }
```

`expected_content` may be omitted for a position request. Occurrence and symbol
anchors always identify a source version. A `SourceAnchor` is:

```json
{
  "path": "src/app.rs",
  "content": "<BLAKE3 digest>",
  "span": { "start": 42, "end": 48 }
}
```

An occurrence span must identify an exact identifier token. A position can be
inside that token. A `SymbolRef` is:

```json
{ "language": "rust", "declaration": SourceAnchor,
  "name_span": Span, "kind": "struct" }
```

Its declaration anchor covers the symbol's extent; `name_span` identifies its name
in the same file. It is not a persistent identity across edits or moves. A client's
symbol request is checked against the captured declaration facts.

The answer has a `snapshot` digest and an `outcome` tag:

```json
{ "snapshot": "…", "outcome": "resolved", "target": SymbolRef,
  "evidence": { "addresses": [ Address, … ] },
  "preview": {
    "container": SymbolRef, "declaration": Match,
    "source": { "path": "src/engine.rs", "text": "…",
                "highlights": [ Highlight, … ], "symbols": [ Symbol, … ] },
    "selection": Span, "identifiers": [ SourceAnchor, … ]
  } }
{ "snapshot": "…", "outcome": "ambiguous", "candidates": [
  { "target": SymbolRef, "declaration": Match,
    "evidence": { "addresses": [ Address, … ] } }, … ] }
{ "snapshot": "…", "outcome": "unavailable", "reason": "unsupported_context" }
```

The target and display container differ for an enum variant: the target is the
variant; the container is its enum. All ranges are absolute half-open UTF-8 byte
ranges. The preview supplies a **complete captured file**, not a clipped excerpt;
clients render the container's range and can scroll through it without another
read. Source, highlights, selection, declaration, and identifier anchors refer to
the same content version. Identifier anchors are not pre-resolved links.

Evidence lists the initial binding address and subsequent re-export addresses.
Self-resolution and lexical resolution have an empty address list. Candidates are ordered by relative
path and name offset; their declaration match IDs support explicit selection.
Clients may alternatively send a candidate's `SymbolRef` as a new origin.

Unavailable reasons are `no_identifier`, `unresolved`, `unsupported_context`,
`external_source_unavailable`, and `cyclic_imports`. Syntax-only navigation is
conservative: Rust supports module/type bindings plus parameters, tuple/slice
bindings, closure captures, generic type parameters, locals, and local type/function
items with modeled scopes. TypeScript supports named/default imports, namespace
qualified types, aliases/re-exports/local export lists, generic type parameters,
and simple function/method parameters. Unsupported scopes
and patterns do not fall back to a same-named outer declaration. Default exports
are not named exports. See the [guide](guide.md) for remaining coverage limits.
The unversioned oracle is not consulted by this capability.

Navigation sources can include `parameter` and `type-parameter` symbols in addition
to the ordinary declaration kinds. These navigation-only symbols do not expand
search results, references, or rename scope.

Invalid ranges or forged symbol identities return `bad_request`; out-of-range
positions return `no_such_position`; changed source returns `stale`; exhausted
traversal budgets return `incomplete`. I/O failures remain errors. These failures
are not converted into an unavailable outcome.

The snapshot identifies consulted source versions and manifests plus the captured
workspace file set. Navigation reuses graph facts and scope caches under the
engine's retention policy and revalidates consulted contents before returning.
It invalidates the retained graph after detecting stale contents. This does not
lock out external editors or promise an atomic filesystem snapshot. There is no
persistent navigation-result cache; each request resolves its exact occurrence.

### Semantic navigation providers

Library hosts can call `NavigationQuery::execute_with(&engine, &provider,
&cancellation)`. Normal `Engine::run`, CLI, TUI, and `serve` requests remain
syntax-only. There is no built-in language-server adapter. The independent
`NavigationProvider` contract leaves the existing `Oracle` API unchanged.

- `ProviderVersion` has nonempty `provider` and `revision` strings. The revision
  must change for every semantic input change, including build options and external
  dependencies.
- `SemanticRequest` carries that version, a `SourceAnchor`, and the complete exact
  source. All spans are absolute half-open UTF-8 byte ranges. A language-server
  adapter must convert its negotiated position encoding against these bytes.
- `SemanticReply` echoes version and origin, lists every additional consulted
  workspace source/configuration as `{path, content}`, and returns the complete
  target set. Workspace targets carry a `SymbolRef`; external targets carry
  `{uri, content, span}`. The engine never fetches provider URIs.
- Workspace targets must match an extracted declaration's language, content, kind,
  extent, and name span. Paths must be workspace-relative, with no parent traversal.
  Multiple targets remain candidates; explicit selection uses their match IDs.
  Any external target, including a mixed workspace/external set, returns
  `external_source_unavailable` rather than confirming a partial set.
- Confirmed semantic results add `evidence.semantic: {provider, revision}`; syntax
  results omit it. Snapshot identity includes the consulted provider revision and
  dependency contents. Sources and revision are checked again before returning.
- `SemanticFailure::Unavailable` retains the syntax outcome. `Incomplete` returns
  `incomplete`, and cancellation returns `cancelled`. Changed revisions or sources
  return `stale`; malformed origins or target identities return `bad_request`.
  Replies are limited to 1,024 targets and 1,024 additional dependencies.

Providers must bound their work, cooperate with `NavigationCancellation`, and never
re-enter the engine while answering under its operation guard. Cancellation checks
cannot interrupt an uncooperative blocking provider. The engine only accepts
workspace declarations represented by language facts; generated declarations and
external source previews require further source-provider support.

## `vvv search`

```json
{ "query": { "pattern": "…", "kind": "…", "symbol": "…", "name": "…", "language": "…" },
  "matches": [ Match, … ],
  "skipped": [ { "language": "typescript", "reason": "invalid pattern: …" } ] }
```

`matches` are ordered declarations first, then by path and position. All `query`
fields optional; at least one of `pattern`, `kind`, `symbol`, `name` is required or
the command errors. A `pattern` that is a single identifier token matches
every identifier spelling it regardless of node kind (a name search); anything else is
matched structurally by ast-grep, node kind included.

A pattern is written in one language. A language whose grammar cannot compile it is
asked once, before any file is read, and listed in `skipped` with the grammar's reason;
its files are not searched and the command still succeeds. `skipped` is absent when
empty. Pass `language` to ask one language only.

## `vvv outline`, `references`, `where`, `deps`, `explain`

Read-only answers built from the same declarations and imports a rename uses. Same-file
imported alias chains resolve to the same addresses in dependencies, explanations
and references. Each is a plain structure of the shared types; `Symbol` fields are
flattened into an outline item. On `explain`, an exact import path under `position`
takes precedence over grouped-statement containment, including nested groups.
Outside entry spans, the containing statement's first grouped entry remains the
fallback.

```json
{ "path": "src/plan/mod.rs", "module": Address,
  "items": [ { "kind": "struct", "name": "Plan", "name_span": Span, "span": Span, "extent": Span,
               "visibility": { "span": Span, "text": "pub" },
               "start": Position, "end": Position,
               "address": Address, "reach": Reach }, … ] }

{ "name": "Plan", "declarations": [ Match, … ], "occurrences": [ Occurrence, … ] }

{ "name": "Plan", "sites": [ { "declaration": Match, "address": Address,
                              "import": "use crate::plan::Plan;" }, … ] }

{ "path": "src/plan/mod.rs", "module": Address,
  "imports":   [ { "span": Span, "path": "crate::edit::ChangeSet", "start": Position,
                   "address": Address, "file": "src/edit/change_set.rs" }, … ],
  "importers": [ { "path": "src/lib.rs", "span": Span, "path": "plan::Plan", "start": Position }, … ] }

{ "path": "src/plan/mod.rs", "position": Position,
  "symbol": Symbol, "declared": Position, "line": "    pub fn new() -> Self {",
  "module": Address, "address": Address, "reach": Reach,
  "via": [ Address, … ], "import": Dep,
  "importers": [ "src/lib.rs", … ] }
```

**Address** is `{ "package": "vvv_core", "path": ["plan", "Plan"] }` — the package as
`use` paths name it, then the module path within it. An import's `path` is the path as
written (`crate::edit::ChangeSet`, `../x/y`); a grouped entry's `group.prefix` is the
enclosing groups' path without a trailing separator (`crate::edit` for
`use crate::edit::{ChangeSet, Edit}`). **Reach** is who may name a
declaration: `"everyone"`, `{ "package": "vvv_core" }`, or `{ "within": Address }`
(that module and its descendants). `module` (on outline, deps, explain) is the file's
own address, absent when the language has none. `address` is absent when no path
reaches the declaration in its language (methods, fields, variants); `import` is absent
without `--from`. `declared` and `line` are where the enclosing declaration's name
starts and the source line holding it, absent outside any declaration. Imports the
layout cannot follow have no `address`/`file`; an import's `address` is what it spells,
so one through a re-export names the re-exporting module, and its `origin` is the
declaration that re-export chain leads to when that is somewhere else (`file` is the
origin's). On `explain`, `via` is every other address a re-export offers the
declaration at (omitted when none) and `import` the import statement at the position,
as a `deps` entry, present only inside one. An import that re-exports what it brings
in (`pub use`, `export … from`) carries `"reexport": true`; one bound under another
name (`use a::B as C`) carries `"alias": "C"`.

## `vvv surface`, `impact`, `dead`, `imports`

Answers about the whole tree. A **Placed** declaration is a `Symbol` flattened with
where it is: `{ "path": "src/plan/mod.rs", "kind": "struct", "name": "Plan", …,
"start": Position, "address": Address, "reach": Reach }`.

```json
{ "package": "vvv_core",
  "items": [ { …Placed, "via": [ Address, … ], "importers": 4 }, … ] }

{ "name": "Plan", "address": Address,
  "consumers": [ { "module": Address, "path": "src/workspace/mod.rs",
                   "depth": 1, "through": Address }, … ] }

{ "items": [ { …Placed, "unsure": 2 }, … ] }

{ "path": "src/plan/mod.rs",
  "unused":     [ { "path": "src/plan/mod.rs", "span": Span, "path": "std::io::Read",
                    "start": Position, "address": Address }, … ],
  "unresolved": [ ImportSite, … ],
  "redundant":  [ ImportSite, … ],
  "unplaced":   [ "tests/plan.rs", … ] }
```

`surface` lists a package's public declarations and any a re-export offers on
(`package` is absent when every package was asked); `via` is every address a re-export
chain offers the item at, omitted when there is none, and `importers` counts files
importing any of its addresses. `impact` lists each module once at the first `depth` it
is reached (1 imports the declaration or an alias of it; 8 at most), `through` being
the module that carried it there — the declaring module at depth 1. `dead` lists
declarations with no resolved token other than their own name; `unsure` counts tokens
vvv could not judge. `imports` sites carry the import's own fields flattened;
`address` is absent for an unresolved site; `unplaced` is the files whose language has
a layout but which it cannot place, so their imports were not judged; `path` at the
top is the file asked about, absent when every file was.

Mutation results always carry `applied`. A preview has `applied: false` and omits
`history_id`; a successful apply has `applied: true` and its required `history_id`.
The Rust result types deserialize these into one preview/applied state and reject
contradictory pairs. A null `history_id` is accepted for previews, as is an omitted
one. Query results cannot become executable plans; results received over the wire
contain no in-process plan to apply.

## `vvv rewrite`

```json
{ "intent": { "query": { … }, "template": "bar($$$ARGS)", "selection": { "ids": [ "…" ] } },
  "applied": false,
  "files": [ FileChange, … ] }
```

`selection` is omitted when every match is selected; it is `{ "ids": [ … ] }` or
`{ "ordinals": [ … ] }` (1-based positions in the search's `matches`). `applied` is `true` only when
`--apply` was passed and the write succeeded; `history_id` is then the entry the apply
made (`"history_id": 3`), what `undo` reverses. Every mutating result (`rewrite`,
`rename`, `move`, `move --symbol`, `batch`) carries the pair.

## `vvv rename`

```json
{ "intent": { "name": "Point", "to": "Vec2", "symbol": "struct", "language": "rust",
              "selection": { "ids": [ "…" ] } },
  "applied": false,
  "declarations": [ Match, … ],
  "occurrences":  [ Match, … ],
  "files": [ FileChange, … ] }
```

- `declarations`: where `name` is declared (filtered by `symbol` if given). More than
  one means the rename is ambiguous.
- `occurrences`: every identifier token spelling `name` in files of the declaring
  language(s), each a `Match` plus `"confidence"` and `"reason"`. `confidence` is
  `"resolved"`, `"unresolved"` or `"other"`; `reason` is the ground for it:
  `declaring` (written in the declaring module), `imported`, `path` (the tail of a
  qualified path resolving to the target), `opened` (a glob import), `re-export`
  (reached through a `pub use` chain vvv followed to the declaration), `oracle`
  (syntax could not place it; an oracle the host provides — a build, a language
  server — said it is the declaration) → resolved; `unresolved` (a bare name nothing
  imports, or a path whose head cannot be placed), `by-name` (no declaration to judge
  against: a method or field) → unresolved; `other-declaration`, `external` (a package
  outside the workspace), `oracle-other` (an oracle said it is another declaration) →
  other. The list
  is ordered by reason, so verdicts are contiguous: resolved, then unresolved, then
  other. Their ids, or 1-based positions, are what `--select` accepts. The
  declaration's own name token is among them.
- `intent.declared_in` names the declaring file when several path-addressable
  declarations share the name; without it such a rename is an error listing them.
- With `selection` omitted, `files` cover resolved occurrences, plus unresolved ones
  only when no competing declaration exists; `other` is never renamed unselected.
  Methods, fields and variants have no target (types would be needed): every
  occurrence is `unresolved` / `by-name` and all are renamed.
- `files` reflect the selection, `occurrences` do not.

## `vvv move`

```json
{ "intent": { "from": "./src/util/parse.rs", "to": "src/net/parse.rs" },
  "applied": false,
  "from": "src/util/parse.rs",
  "to": "src/net/parse.rs",
  "from_address": Address, "to_address": Address,
  "notices": [ Notice, … ],
  "respellings": [ { "path": "src/lib.rs", "span": Span, "start": Position,
                     "from": "crate::util::parse::X", "to": "crate::net::parse::X" }, … ],
  "files": [ FileChange, … ] }
```

`from`/`to` are the normalised, workspace-relative paths actually used; either may be
a directory; `from_address`/`to_address` are the module addresses when the language has
them. `notices` and `respellings` are omitted when empty. A **Respelling** is one
reference rewritten in place — its `span` is the edit in `files` that does it; an edit
no respelling accounts for is structural (a `mod` line moved, a visibility widened).
Every moved file appears in `files` with `moved_to` set even if its contents do not
change — a directory move lists each file.

## `vvv move --symbol`

```json
{ "intent": { "name": "Config", "from": "src/util.rs", "to": "src/config.rs" },
  "applied": false,
  "from": Address, "to": Address,
  "notices": [ Notice, … ],
  "respellings": [ Respelling, … ],
  "files": [ FileChange, … ] }
```

`from`/`to` are the declaration's addresses before and after; `respellings` are the
consumers rewritten in place, as for a file move. The Intent is
`{ "command": "move_symbol", "name", "from", "to" }`. Notices: `unreachable` as for a file
move; `redundant_import` (`import`) when the destination imported the declaration inside
a grouped statement vvv does not split for this.

## `vvv batch`

Input: a JSON array of Intents (below), from a file or stdin. Each is planned against
the tree as the previous one leaves it, so a rename may follow the move of the file it
touches.

```json
{ "intents": [ Intent, … ],
  "applied": false,
  "notices": [ Notice, … ],
  "files": [ FileChange, … ] }
```

`files` shows every touched file before the first step against after the last, at its
final path; `edits` is empty there, since each step's edits are in the coordinates of
the state before it. Applying is one transaction: if a step no longer holds against the
real tree, recovery restores earlier effects or returns `recovery_failed` naming
remaining effects and unverified paths. Recovery also compensates failed history
saves (see [Errors](#errors)). History records
one entry, `{ "command": "batch", "intents": [ … ] }`, and one `undo` reverses it all.

## Intent

Every mutating command's request, as one tagged value. It is what `history` records,
what `batch` takes, and what a remote client sends.

```json
{ "command": "rename", "name": "Config", "to": "Settings", "symbol": "struct" }
{ "command": "rewrite", "query": { … }, "template": "bar($$$A)", "selection": { "ids": [ … ] } }
{ "command": "move", "from": "src/util/parse.rs", "to": "src/net/parse.rs" }
```

## `vvv history`

```json
{ "entries": [ { "id": 1, "at": 1789000000, "intent": Intent, "files": 2,
                 "paths": [ "src/lib.rs", "src/util/parse.rs" ],
                 "moves": [ [ "src/util/parse.rs", "src/net/parse.rs" ] ] }, … ] }
```

Oldest first; the last entry is what `vvv undo` reverses. `at` is seconds since the
Unix epoch. `paths` are the files the apply wrote, at their pre-apply paths — what undo
restores; `moves` the files it moved, as `[from, to]` — what undo reverts. Both are
omitted when empty.

## `vvv undo`

```json
{ "undone": { "id": 2, "at": …, "intent": { "command": "move", … }, "files": 5, … },
  "restored": [ "src/lib.rs", "src/util/parse.rs", … ],
  "moves_reverted": [ [ "src/util/parse.rs", "src/net/parse.rs" ] ] }
```

`restored` lists pre-apply paths. Undo refuses (`… changed since it was written;
refusing to undo`) if any file the apply wrote differs from what it wrote, and keeps
the history entry so the situation can be fixed by hand and retried.

## The two-step flow

1. Run the command without `--apply`. Inspect `files[].diff` and the ids.
2. Re-run with `--select id,id,…` (optional) and `--apply`.

Separate CLI invocations recompute the search and plan. An ID includes the
match's path, byte span, and text; an ID absent from the recomputed results returns
`no match with id(s) …`. Edits elsewhere in the file do not necessarily change that
ID. Ordinals select the current result order and can identify different matches
after source changes; a client retaining selections should prefer IDs.

A Rust client can retain `Planned<T>` and apply it without replanning. Apply
refuses a witnessed source whose contents differ from its planning snapshot
(`… changed since the plan was made`).
Source fingerprints come from the snapshots used by edit and move producers, so
changes before the initial preview are refused too. Resolution-only inputs are not
included. A move checks destination absence before writing, with a same-entry
exception for case-only renames; other occupied destinations return `exists`.
Preflight does not reserve paths. Forward and recovery moves also preserve
any destination created after the check. Case-only moves use two such operations
through a unique name and are not atomic as a whole.

## Errors

`code` is one of these and does not change between releases; `message` says what
happened in words and `hint` (when present) what to try, and neither is for parsing:

| code               | meaning                                                                           |
| ------------------ | --------------------------------------------------------------------------------- |
| `output_limit`     | the complete result exceeds the requested result-byte budget                      |
| `cancelled`        | navigation was cancelled                                                          |
| `incomplete`       | navigation or its provider could not complete within its budget                   |
| `bad_request`      | the line is not JSON, or not a known command (`vvv serve`)                        |
| `bad_query`        | a search with none of pattern, kind, symbol, name                                 |
| `bad_pattern`      | a pattern or node kind the language's grammar rejects                             |
| `bad_selection`    | a `--select` that could not be read, or ids the search did not find               |
| `bad_template`     | a template naming a capture the match does not have                               |
| `no_such_symbol`   | no declaration by that name (and kind)                                            |
| `ambiguous_symbol` | several declarations share the name; `declared_in` picks one (the hint names one) |
| `no_language`      | no registered language claims the file                                            |
| `no_layout`        | the language has no layout: paths cannot be followed, files not moved             |
| `no_such_position` | a position past the end of the file                                               |
| `exists`           | the destination already exists                                                    |
| `not_found`        | a file, or the file declaring a name, could not be found                          |
| `unmovable`        | the layout refuses the move: a root, across packages, into itself                 |
| `conflict`         | two edits of one plan overlap, or a file is moved twice                           |
| `stale`            | a source anchor, navigation input/provider revision, plan, or undo source changed |
| `no_history`       | nothing to undo, or a history file that cannot be read                            |
| `io`               | reading or writing the tree failed                                                |
| `recovery_failed`  | recovery could not restore or verify all attempted effects                        |

A failed file mutation whose recovery cannot restore or verify every attempted
effect returns `recovery_failed` with an additional `recovery` object. Other errors
omit this field. The envelope schema is 1.

`recovery` contains:

- `cause`: the initiating `Failure` (`code`, `message`, and optional `hint`).
- `failures`: failed recovery operations, each with `operation`, project-relative
  `path`, `code`, and `message`. Operations are `restore_file`, `restore_move`,
  `restore_directory`, `remove_file`, and `remove_directory`.
- `remaining`: confirmed differences from the before-state, each with `path`,
  `expected`, and `observed`.
- `unverified`: paths whose final state could not be read, each with `path`,
  `expected`, `code`, and `message`. An unverified path is never claimed restored.

A state is `{ "kind": "absent" }`, `{ "kind": "file", "fingerprint": "…" }`,
`{ "kind": "directory" }`, or `{ "kind": "other" }` (for example, a symlink).
File fingerprints are full hexadecimal BLAKE3 content hashes, not file contents.
For a case-only move, a file state also carries an optional `spelling` relative
path: the directory entry's stored filename. Expected and observed spellings can
differ even when the fingerprints match. Unrestored temporary files appear as
their own paths in `remaining`; they are never hidden inside an error message.
The lists are in deterministic recovery or path order. Recovery attempts continue
for independent effects after a failure. If the final before-states are all verified,
the initiating error is returned instead, even if a restoration operation returned
an error after completing its effect.

These results describe recovery from returned errors, not panics or crashes.
There is no durable recovery journal or isolation from concurrent writers.
Apply, batch, and undo include ledger-save compensation: an unrestored `.vvv/history.json` is
listed like any other remaining file. An unrestored owned directory is listed
as a directory; `restore_directory` identifies a failed attempt to recreate one
removed during undo. During failed undo, expected states describe the pre-undo
(applied) state, including the original ledger with the entry still present.
Directory ownership is stored in internal history receipts and is absent from
client answers. A receipt without this field defaults to no owned directories,
so undo retains directories without ownership evidence.

The Rust library dispatcher returns an in-process `Execution` so a mutation preview
can retain its executable plan and an applied completion can retain its committed
history id. Interfaces consume `Execution::into_answer()` before serializing the
wire `Answer`. Executable plans and committed completion handles are not
serialized.
