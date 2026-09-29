# JSON protocol

Pass `--json` to any command. Output is a single JSON document on stdout, including
errors, so a client never needs stderr. Exit code is `0` for `ok`, `1` for `error`.
Or run `vvv serve` and send the same commands as JSON, one per line (below).

The wire types are exported by `vvv-engine`. Shared types live in `protocol/`;
capability-specific requests and answers live with their implementations and are
exported at the crate root. See [architecture.md](architecture.md) for Rust import
paths. A Rust client needs only `vvv-engine`. This page specifies serialization. Field order is not significant. Optional fields follow the per-command contracts
below; continuation and expansion handles use null to signal completion.

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
{ "command": "schema", "for_command": "context", "contract": "arguments" }
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
`schemas_available` says whether this build includes generated schemas. If enabled,
each command also includes `schemas: { arguments, request, result, response }`,
with the four schema identifiers. Fetch them with `schema` below. Without schema
support, these references and the schema command are absent. Language registration
is not a guarantee that every construct is resolvable. `id` and `max_output_bytes`
belong to the call envelope rather than individual command parameter lists.

## `vvv schema`

Available with the default CLI `schemas` feature, or the engine's optional `schema`
feature. The query performs no workspace reads. It accepts `contract` and an
optional `for_command`:

```json
{ "command": "schema", "for_command": "context", "contract": "arguments" }
{ "command": "schema", "for_command": "navigate", "contract": "response" }
{ "command": "schema", "contract": "call" }
{ "command": "schema", "contract": "reply" }
```

| Contract    | Requires `for_command` | Describes                                                  |
| ----------- | ---------------------- | ---------------------------------------------------------- |
| `arguments` | Yes                    | Command-specific input fields                              |
| `request`   | Yes                    | Command inputs including the `command` discriminator       |
| `result`    | Yes                    | Successful command payload                                 |
| `response`  | Yes                    | That command's success/error response envelope             |
| `call`      | No                     | Any session request, including `id` and `max_output_bytes` |
| `reply`     | No                     | Any session response, including its optional echoed `id`   |

An invalid command/contract combination returns `bad_request`. Unknown command
names and contract names fail request decoding. The successful result is
`{ "id": "urn:vvv:schema:…", "document": { … } }`. `document` is a JSON Schema
Draft 2020-12 object with `$schema`, `$id`, and any needed local `$defs`.

Identifiers take the form
`urn:vvv:schema:1:<command-or-session>:<contract>:<digest>`. The BLAKE3 digest covers
the compact serialization of the key-sorted document before adding `$id`. No remote
resolution is required; these identifiers name documents, not download URLs.
Identifiers can change for additive contract updates or description changes. The
envelope's `schema: 1` retains its existing compatibility meaning and is distinct
from the JSON Schema dialect and document identity.

Inputs reflect Serde defaults and accepted unknown fields, with explicit budget
ranges; strict objects such as context budgets reject unknown fields. Outputs
describe serialization rather than input defaults. Mutation results require
`history_id` exactly when `applied` is true. Untagged result unions use `anyOf`,
since some result shapes overlap. Source paths/IDs/module paths remain strings,
and nullable fields retain their actual representation.

Schemas cover wire shape and expressible constraints. Successful validation does
not guarantee a valid query meaning, current source version, existing path, valid
in-file range, or permitted output-budget policy for a mutation. Those are still
engine checks. Schema retrieval itself obeys a session call's output budget and
can return `output_limit`; request an individual contract instead of an aggregate
or increase the limit.

Artifacts live in `docs/schemas/v1/`. Regenerate and format them with:

```console
cargo run -p vvv-engine --features schema --example schemas -- --write
dprint fmt docs/schemas/v1
```

Run the same example without `--write` to check artifact content against the
current types. CI runs that check and dprint; schema tests independently validate
the documents, requests, and responses, and the corpus validates actual command
output against per-command response schemas. JSON formatting does not affect
artifact identity; the catalog hashes canonical generated data.

## `vvv context`

The request accepts the same `origin` and `selection` as `navigate`, an optional
`budget`, `detail` (`"body"` by default or `"signature"`), `references` (default false),
and `include_enclosing` (default false).
Budget fields default independently:

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
- `complete`: whether the requested detail is fully present.
- `signature`: present only in signature mode, either
  `{ "outcome": "available", "requested": <SourceAnchor> }` identifying the full
  signature range, or `{ "outcome": "unsupported" }`.

Signature ranges include attached comments/documentation and attributes, followed
by the exact AST declaration header, excluding its implementation/member body and
trailing whitespace before that body. Parameters, generic constraints, return types,
and braces inside types remain intact. Bodyless supported declarations (including
type aliases and tuple structs) retain their complete extent. Signatures are source
excerpts, not generated summaries or inferred types. Unsupported forms return empty
`text`, a zero-length `excerpt` at the declaration start, and `complete: false`;
this does not increment the byte-limit omission count. Request body detail to read
the declaration. `target.declaration` always identifies the full declaration.

In signature mode, outgoing navigation examines only identifiers in the seed's
signature. Related items and an explicitly included enclosing item use the same
detail. Incoming reference scanning is unchanged. A one-shot signature can be
expanded by requesting body detail with the returned target as a symbol origin.

The optional `enclosing` field identifies the nearest enclosing declaration as a
`SymbolRef`. It is included at the requested detail only when `include_enclosing: true`; the same
owner is not reintroduced through outgoing or incoming relationships by default.
Items are ordered seed first, the optional enclosing body second, outgoing
occurrences in source order, then incoming files/tokens in path/source order. Each target
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

## Paged queries and exact source expansion

`context_page` shares context's `detail` and `include_enclosing` options and returns the same
optional `enclosing` location on each page, including empty progress pages.

`search_page`, `context_page`, `continue`, and `expand` are read-only session and
library commands. Keep the same `serve` process (or clones of one `Engine`) for
all continuations. Existing `search`, `context`, and their CLI flags retain their
one-shot behavior.

```json
{"command":"search_page","query":{"pattern":"Engine"},"page":{"max_items":20,"max_bytes":8192}}
{"command":"context_page","origin":{"kind":"position","path":"src/engine.rs","position":{"line":8,"column":15}},"references":true,"page":{"max_items":2,"max_bytes":4096},"work":{"max_lookups":16,"max_files":8}}
{"command":"continue","cursor":"<next_cursor>","page":{"max_items":20,"max_bytes":8192}}
{"command":"expand","cursor":"<item.expansion>","max_bytes":4096}
```

`context_page` accepts `selection` (default `"all"`) and `references` (default
false). Page and work objects default field by field and reject unknown fields:

| Object | Field         | Default | Range        |
| ------ | ------------- | ------- | ------------ |
| `page` | `max_items`   | 20      | 1–64         |
| `page` | `max_bytes`   | 16384   | 1024–1048576 |
| `work` | `max_lookups` | 64      | 1–512        |
| `work` | `max_files`   | 64      | 1–1024       |

`continue` accepts optional `work` for context cursors; search cursors reject it.
`expand.max_bytes` is required, with the same byte range. The smaller of the
command's byte budget and `Call.max_output_bytes` is applied before generating a
result. Counts include the entire compact result JSON: metadata, escaping, and
cursor strings. The response envelope and echoed ID are excluded.

Search results contain `kind: "search"`, `snapshot`, `query`, `items`, `skipped`,
`total_items`, and `next_cursor` (null at the end). Each item is a complete `Match`
with an additional absolute one-based `ordinal`. Declarations-first path/source
ordering, match IDs, captures, and skipped-language diagnostics match unpaged
search. Ordinals never restart at one for a new page. An indivisible match that
cannot fit returns `output_limit`; no match is skipped or truncated.

Context results contain `kind: "context"`, `snapshot`, the existing context
`outcome`, `items`, `references_by_name`, `work: {lookups, files}`,
`unresolved: {ambiguous, unavailable, no_container}`, `traversal_complete`, and
`next_cursor`. Each item has the existing `ContextItem` fields plus `expansion`,
an independent excerpt cursor or null. Signature items also carry `body_expansion`,
an independent cursor starting at the full declaration's beginning, including when
signature extraction is unsupported. It is absent for body-detail items. Ambiguity candidates remain complete and
indivisible. `continue` returns the same operation-specific shape, distinguished
by `kind`.

Context traversal preserves seed, optional enclosing body, outgoing occurrence,
and incoming path/source order, with first-evidence deduplication across pages.
Work counters describe additional relationship work in this call; the initial
seed lookup is excluded. Resuming within an incoming file counts that file once
in the new call. Deferred traversal is represented by `next_cursor`, independently
of irreducible `unresolved` counts. Existing one-shot omission counters are not
redefined. A page can be short or empty when work progresses without yielding a
new declaration. A terminal empty page can confirm exhaustion; a nonterminal empty
page always advances the frontier. Complete incoming evidence is still limited
to the exact spelling, and test-path relationships do not establish test coverage.

Expansion results contain `snapshot`, `target`, `requested` (the full range of the
selected signature or declaration), `excerpt` (the returned anchored range), `start`, exact `text`, `done`,
and `next_cursor`. Concatenating the initial item text and its expansion chunks
reconstructs the requested signature or declaration, including documentation and
indentation. With `body_expansion`, concatenate expansion chunks alone: the handle
starts at the declaration beginning, so appending them to a signature duplicates it.
Ranges are absolute half-open UTF-8 bytes; `start` is a zero-based line/character
position. No ellipses or formatting are inserted. `done` marks the final chunk;
an item's `complete` means it contains the entire requested detail. Continuing
relationships does not consume an excerpt cursor, or vice versa.

Cursors are opaque, retryable, process-local handles. The same retained cursor
and budgets return the same page and successor; changing budgets starts at the
same logical position. `continue` rejects excerpt cursors and `expand` rejects
query cursors. These tokens are neither authentication credentials nor persistent
bookmarks. Query scope, selection, snapshot, and ordering cannot change through a
continuation. Failed validation or budget checks do not advance the checkpoint.

Paged queries capture a sorted workspace inventory, registered language identities,
all claimed source content hashes (including unmatched files), layout manifests,
and workspace `.ignore`, `.gitignore`, and `.git/info/exclude` inputs. A fresh walk
also observes the effects of ancestor/global ignore configuration on inclusion.
They bypass graph stamp/trust caches, load a fresh graph on each call, and revalidate
before publishing. Additions, deletions, renames, source changes, and resolution
configuration changes invalidate the query tree. Internal apply/undo invalidates
queries even when attempted writes fail. This detects observed changes without
locking external editors or promising filesystem transaction isolation.

Retention limits are 16 query trees, 32 MiB charged per query, 128 MiB total, and
a fixed ten-minute lifetime from capture, measured monotonically. Charges
conservatively include allocation overhead for inventory, query data, checkpoints,
and tokens; no declaration bodies or full workspace graphs are retained by the
cursor store. Least recently used whole query trees are evicted when needed.
Retained-state limits do not bound parser allocations, temporary graph memory,
execution time, or validation I/O. Whole-workspace validation is performed even
for small pages. Discovery publishes `page_defaults`, `page_maximum`,
`work_defaults`, `work_maximum`, and `query_retention`.

Failures carry structured `continuation` recovery actions:

| Code              | `continuation`                    | Meaning                                                  |
| ----------------- | --------------------------------- | -------------------------------------------------------- |
| `stale`           | `restart_query`                   | Inputs changed; restart from a current origin            |
| `cursor_expired`  | `restart_query`                   | State expired, was evicted, or belongs to another engine |
| `invalid_cursor`  | `correct_cursor`                  | Malformed, unknown checkpoint, or wrong cursor kind      |
| `output_limit`    | `increase_budget_or_narrow_query` | Increase the byte budget or narrow the query             |
| `retention_limit` | `narrow_query`                    | Retained query/checkpoints exceed limits                 |

`output_limit` includes `max_bytes`, `required_bytes`, and an optional `anchor`
identifying an item that cannot fit. An ambiguous candidate set is never shortened
into apparent certainty. Exceeding retention limits does not consume an existing
cursor. After a stale result, all query and excerpt cursors in that tree are invalid.

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

## `resolve`

The compact companion to `navigate` accepts the same `origin` and `selection`.
`ResolutionQuery` returns `ResolutionReply` with the same snapshot, resolution
semantics, source validation, and evidence. The CLI exposes it through
`navigate --compact`; MCP's `vvv_navigate` maps to this command.

Resolved replies contain `outcome: "resolved"`, `target`, `id`, `name`, `start`,
`evidence`, and `container`. `target` and `container` are versioned `SymbolRef`s;
`start` is the declaration's zero-based starting position. `container` preserves
preview containment (for example, the enum containing a variant). Request the
target through `context`/`context_page` to obtain source.

Ambiguous replies keep every candidate as `{target,id,name,start,evidence}` in the
same order and with the same selectable IDs as full navigation. Unavailable
replies keep the same reason. Full source, highlights, identifier lists, and
serialized declaration bodies are omitted by construction, not truncated after
budget checks. Candidate sets and long names can still exceed a caller's budget;
that produces the ordinary `output_limit` failure. Full navigation still owns
source capture internally, so this is a delivery reduction, not a parser-memory
or runtime guarantee.

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

Both `search` and `search_page` accept an engine-owned `scope` beside their plugin
predicates. For unpaged `search`, predicates remain top-level; paged requests nest
them under `query`:

```json
{"command":"search","name":"Engine","scope":{"paths":["crates/vvv-engine/src"],"packages":["vvv-engine"]}}
{"command":"search_page","query":{"name":"Engine"},"scope":{"packages":["vvv_engine"]}}
```

`paths` and `packages` default to empty lists (unrestricted). Path alternatives
match whole components and represent exact files or directory prefixes, using
workspace-relative `/` paths; absolute paths, `..`, backslashes, drive syntax, and
NUL bytes are rejected with `bad_request`. `.` or an empty prefix matches the
workspace. Package alternatives match either a manifest name or canonical package
ID exactly, using the deepest owning package root from the file's language layout.
Unknown packages and files without package ownership do not match. Alternatives
within each list are ORed; the lists are ANDed. There is no heuristic exclusion of
tests or fixtures.

The result echoes nonempty `scope`. Collection and retained-result limits apply
after filtering, and page ordinals/totals refer to the filtered set. Scope is part
of the retained query identity and cannot change through `continue`; delivery
budgets may still change. Source/manifest validation remains workspace-wide.

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
| `cancelled`        | a read operation was cancelled                                                    |
| `incomplete`       | navigation or its provider could not complete within its budget                   |
| `cursor_expired`   | continuation state expired, was evicted, or belongs to another engine             |
| `invalid_cursor`   | malformed or wrong-kind continuation handle                                       |
| `retention_limit`  | retained query state exceeds the advertised limits                                |
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

## Retained mutation plans

Preparation, inspection, review continuation, apply, and discard share the engine's session-owned plan store. They are available to
library clients and through `vvv serve`; MCP maps the same requests to tools.
Only `apply_plan` writes workspace files. These commands do not change existing
`rename`, `rewrite`, or `apply: true` semantics.

```json
{"command":"prepare_rename","intent":{"name":"Engine","to":"Runtime","declared_in":"src/engine.rs"},"max_bytes":16384}
{"command":"inspect_plan","plan_id":"<opaque handle>","max_bytes":16384}
{"command":"apply_plan","plan_id":"<opaque handle>"}
{"command":"discard_plan","plan_id":"<opaque handle>"}
```

`prepare_rename.intent` is a `RenameIntent`: `name`, `to`, optional `symbol`,
`language`, `declared_in`, and `selection`. Preparation and inspection accept
`max_bytes` (default 16384, range 1024–1048576), narrowed by the call's optional
`max_output_bytes`. Without `page`, preparation rejects oversized complete reviews before publishing a
handle. With `page`, the first page must fit before publication. No edits or ambiguity candidates are omitted to fit the output budget.
Apply rejects `max_output_bytes` before consuming the plan or writing; its compact
receipt is size-checked before transaction execution.

Preparation, inspection, and discard return `plan_id`, `lifetime_seconds` (fixed
from preparation, not a remaining-time counter), and a tagged lifecycle:

| `state`     | Additional data                                                    |
| ----------- | ------------------------------------------------------------------ |
| `prepared`  | `preview`: complete existing `Rename` or `Rewrite` preview payload |
| `applied`   | `receipt`: the successful apply result                             |
| `failed`    | `failure`: original structured `Failure`                           |
| `discarded` | None                                                               |

Inspection returns captured evidence, not newly resolved edits or a freshness
verdict. Before application, the engine compares the complete captured input
inventory, contents, manifests, and walk configuration against a fresh capture,
and checks its mutation revision. Existing plan fingerprints and transaction
recovery remain authoritative during writes. Unrelated source edits may invalidate
a plan; external filesystem writes are not locked out. The engine preserves the
configured library `Oracle` during preparation but does not version external
oracle state; applying uses the captured edits rather than asking it again.

Apply returns `{plan_id, history_id, files: [{path, content}]}`. `history_id` is the
committed durable undo entry; `files` contains content identities written by the transaction.
The response is a transaction receipt, not a compiler/test result. Formatting,
compilation, linting, and tests can be requested separately with `validate_plan`. `history` and `undo` keep
their existing stack semantics; undo does not accept a plan handle.

Successful apply is idempotent within the retention window: replay returns its
original receipt even after subsequent edits or undo, without another transaction.
A failed attempt records its structured error and consumes the executable plan;
repeat apply returns `plan_consumed`. Inspect before preparing a replacement,
especially after `recovery_failed`. Discard releases pending executable edits;
it never undoes writes or removes a successful receipt. Discarded tombstones are
retryable until the next preparation reclaims them.

`plan_id` is opaque, process-local, and scoped to one engine and its clones. It is
not authentication, a durable bookmark, or a client-supplied edit payload. Malformed
handles return `invalid_plan`; unknown, expired, or foreign handles return
`plan_expired`; discarded/failed plans return `plan_consumed` on apply. Source
changes return `stale`. Prepare and review a new plan after expiry or staleness.

Discovery's `plan_retention` reports `max_plans: 16`, `max_plan_bytes: 16777216`,
`max_total_bytes: 67108864`, and `lifetime_seconds: 600`. Monotonic expiry is fixed
at preparation and includes terminal outcomes. Retention charges source copies,
edit/review data, snapshots, and allocation overhead conservatively; it is not a
limit on temporary parser/planner memory. `retention_limit` rejects a new plan
without evicting pending plans or successful receipts. Discard pending plans,
narrow the mutation, or wait for expiry. Restarting the session loses handles but
keeps committed undo history.

### Paged plan reviews

`prepare_rewrite` accepts `intent: RewriteIntent` (`query`, `template`, optional
`selection`) and the same `max_bytes`/`page` fields as `prepare_rename`.
Preparation and `inspect_plan` accept optional `page: PageBudget` (defaults
20 items, 16384 bytes; bounds 1–64 items and 1024–1048576 bytes). Omitting `page`
preserves complete-review JSON, including existing rename preview shapes.
The effective page byte limit is the minimum of `max_bytes`, `page.max_bytes`,
and optional `Call.max_output_bytes`. The entire compact result counts, including
escaping and cursor strings. `max_items` counts metadata records and text chunks.

```json
{"command":"prepare_rewrite","intent":{"query":{"pattern":"increment($X, 1)"},"template":"increment($X, 2)"},"page":{"max_items":20,"max_bytes":8192}}
{"command":"inspect_plan","plan_id":"<handle>","page":{"max_items":20,"max_bytes":8192}}
{"command":"review_plan","cursor":"<next_cursor>","page":{"max_items":20,"max_bytes":8192}}
```

Paged preparation and prepared-plan inspection return `PlanReviewPage`, with:

- `kind: "plan_review"`, `plan_id`, fixed `lifetime_seconds`, and `review_id` (the
  content identity of the complete captured mutation preview).
- `mutation: "rename"` or `"rewrite"`, `totals: {declarations, occurrences, files, edits}`.
- `intent`: complete tagged mutation intent on the first page only.
- `items`: ordered metadata/text records; `next_cursor`: continuation or null.

Applied/failed/discarded inspection retains the complete lifecycle shape and latest
validation evidence. Continuation pages contain immutable review content without
live lifecycle or validation fields. They never imply current source validity.

Record sections are `declaration`, `occurrence`, `file`, `edit`, and `diff`.
Each record has `index` (zero-based within its section) and, for edits,
`file_index` (zero-based owning file). Declaration and occurrence order matches the
complete rename preview, so the occurrence selection ordinal is `index + 1`.
Files keep preview order; edits keep the file's existing sorted span order.
Declarations precede occurrences; each file's metadata precedes its edits and diff.

A metadata item is `{kind:"metadata", section, index, file_index?, value}`.
`value` uses the corresponding ordinary preview record's JSON shape:
`Match`, `Occurrence`, file metadata, or `Edit`. File metadata omits `edits` and
`diff`, which are delivered separately. Text fields named `text`, `line`, and
`replacement`, including nested capture and symbol fields, are replaced by empty
strings and delivered as subsequent chunks when nonempty. Other metadata is
indivisible. Unsupported or ambiguous occurrences retain their original verdicts.

A text item is `{kind:"text", section, index, file_index?, field, offset,
total_bytes, text, complete}`. `field` is an RFC 6901 JSON pointer into that record.
Offsets and totals count UTF-8 bytes, not characters or JSON-escaped lengths.
Chunks end on UTF-8 boundaries; `complete` means this is the field's last chunk.
Concatenate chunks in offset order and replace the metadata's empty field.
A diff section has only text chunks for `/diff`; attach the reconstructed string
to its file. Assemble edits by `file_index` and edit `index`. This reconstructs
the complete preview's declarations, occurrences, edits, and exact unified diffs.
Intent and required metadata must fit whole; otherwise `output_limit` gives the
required result size. Budget failures never publish a partial executable plan.

`review_plan` uses opaque `PlanReviewCursor` tokens, distinct from source-query
cursors; `continue` and `expand` reject them, and `review_plan` rejects query tokens.
Tokens identify validated record/text positions without accumulating per-page
checkpoints. Same cursor and budget replay the same page; different budgets start
at the same position. Failed delivery/cancellation advances no state. A review
page may end in the middle of one text field but never discards its remaining bytes.

Review does not walk or revalidate the workspace: source edits, apply, undo, and
failed apply attempts leave captured content unchanged. Apply still performs its
existing stale-input checks. Reviews survive apply/failure until the plan's fixed
expiry. Discard releases pending review data; its cursors return `plan_consumed`.
Expired or foreign review roots return `plan_expired`; malformed or invalid
positions return `invalid_cursor`. Neither pagination nor inspection extends expiry.
Retained review data is charged after apply as well as before it, alongside baseline
and validation evidence. Memory limits still apply even when output is paginated.
Clients decide when review is sufficient; apply does not count fetched pages.

Rust preparation/inspection return `PlanReviewReply::Complete(PlanReview)` or
`::Page(PlanReviewPage)`; `PlanStatus::Prepared.preview` is `PlanPreview` with
shared immutable rename/rewrite payloads. These are Rust API changes, independent
of the preserved default rename JSON. `review_plan` returns `PlanReviewPage`.

## Applied-plan validation

`validate_plan` is a separate capability. It requires an applied, unexpired retained
plan and does not change the receipt or undo history. It runs caller-supplied
programs directly, without an implicit shell, with the disk workspace root as cwd,
null stdin, and the session's environment and permissions. It is **not read-only**
and does not sandbox programs or recover their effects. Use formatter check mode;
validation never chooses commands itself. `discover.validation_available` is true
only for supported Unix and Windows disk workspaces, and `validation_defaults` publishes the default
budget. Virtual workspaces and unsupported platforms return `bad_request` without execution.
Windows resolves extensionless program names as `.exe` through the workspace root
and `PATH`; other extensions are rejected as a spawn failure. To run a batch file,
the caller must explicitly provide its shell program and arguments.

```json
{
  "command": "validate_plan",
  "plan_id": "<handle>",
  "checks": [
    { "name": "format", "program": "cargo", "args": ["fmt", "--check"] },
    { "name": "compile", "program": "cargo", "args": ["check", "--workspace"] }
  ],
  "extra_inputs": [],
  "budget": { "timeout_ms": 60000, "max_bytes": 16384 }
}
```

`checks` has one to four entries. Each `name` is nonempty and at most 128 bytes;
`program` is nonempty, and `args` defaults to an empty array with at most 64 entries.
Program/argument NULs are invalid. Labels, programs, and arguments together are at
most 8192 UTF-8 bytes. `extra_inputs` defaults to empty and accepts at most 32 existing
workspace-relative file paths, without `..`, root, or `.` components. `budget`
defaults to `{timeout_ms: 60000, max_bytes: 16384}`. Deadline range is 1–300000 ms;
result-byte range is 4096–1048576. Invalid shapes/plan states return `bad_request`;
invalid budgets return `bad_request`. Unknown/expired plans use existing plan errors.
Call-level `max_output_bytes` is rejected before execution; use `budget.max_bytes`.

Before launching anything, the engine compares the planner's full input snapshot,
updated with the contents written by apply, to the workspace. Differences return
`stale`. It captures workspace-visible files (including raw binary contents),
planner source/configuration inputs, and explicit extra inputs twice before launch.
The engine's `.vvv` directory is excluded from the walk. Caller-specified inputs
are still explicit. The digest covers paths and complete contents; `input_files`
counts unique inputs. It observes inputs again between commands and after the batch.
Ignored/hidden files not captured this way, tool versions, ambient environment,
installed dependencies, and external services are not versioned. There is no atomic
filesystem snapshot or exclusion of external writers; reverted transient changes
between captures are not observable.

The result is a `ValidationReport`:

| Field                              | Meaning                                                            |
| ---------------------------------- | ------------------------------------------------------------------ |
| `plan_id`, `history_id`, `sources` | Applied receipt identity and written file versions                 |
| `run`                              | Starts at 1 and increases for each recorded run on this handle     |
| `before`, `after`                  | Input snapshot digests; `after` is null when capture fails         |
| `input_files`, `extra_inputs`      | Initial unique input count and caller's extra paths                |
| `source_state`                     | `unchanged`, `changed`, or `unavailable`                           |
| `passed`                           | Every check passed and the observed inputs stayed unchanged        |
| `checks`                           | Ordered command descriptions and results, including checks not run |

Each check includes `command: {name, program, args}`, `outcome`, nullable
`exit_code`, `duration_ms`, `stdout`, `stderr`, and nullable `failure`. Outcomes are
`not_run`, `passed`, `failed`, `timed_out`, `cancelled`, or `error`. A nonzero exit
is `failed`; spawn/capture/wait/termination failures are `error`, with typed
`failure: {operation, os_code}` (the OS code can be null). Signal exits can have
null exit codes. Each output is `{text, bytes_seen, truncated, complete}`: lossy
UTF-8 text, total bytes observed before capture stops, whether the stored prefix
was shortened, and whether EOF was reached. JSON escaping counts against the result
budget. Logs can shrink at UTF-8 boundaries; statuses and identities never disappear.
Oversized metadata or retention capacity fails before any program is launched.

Commands execute sequentially; a command failure does not skip later checks.
Changed/unavailable inputs stop the batch. The deadline covers the batch including
between-command input captures; initial/final capture and cleanup are additional
latency. Individual filesystem/parser operations are not preempted. Cancellation or
timeout kills the active Unix process group and reaps its leader; output drain has
a bounded grace period. This is process cleanup, not containment of programs that
escape their group. No later program launches after cancellation or deadline expiry.

A completed report is retained as optional `PlanReview.validation`, replacing the
previous report within the existing memory and fixed expiry limits. Each validation
request reruns checks; it is not idempotent. Cancellation during execution still
records the evidence before returning a cancellation error; use `inspect_plan` after
an interrupted response. Cancellation before execution can leave no new report.
The original apply receipt remains retryable and unchanged. Failed checks are an
`ok` protocol result with `passed: false`, not an apply failure. Inspection returns
historical evidence and does not recheck current sources.

## MCP stdio adapter

`vvv -C <root> mcp` is an optional CLI adapter over the same in-process engine.
The `mcp` feature enables `schemas` and the pinned official Rust SDK (`rmcp 1.7.0`).
The supported and tested protocol baseline is `2025-11-25`; other versions are
rejected during initialization. `serve` retains its existing JSON-lines contract.
Only MCP messages go to stdout, including when `--json` is supplied; startup
errors go to stderr. The workspace is fixed at launch.

`tools/list` returns all fourteen tools in one response, with generated JSON Schema
inputs and per-command `Response` output schemas. A tool-list cursor is invalid.

| MCP tool              | Engine request    |
| --------------------- | ----------------- |
| `vvv_discover`        | `discover`        |
| `vvv_search`          | `search_page`     |
| `vvv_navigate`        | `resolve`         |
| `vvv_relationships`   | `relationships`   |
| `vvv_context`         | `context_page`    |
| `vvv_continue`        | `continue`        |
| `vvv_expand`          | `expand`          |
| `vvv_prepare_rename`  | `prepare_rename`  |
| `vvv_prepare_rewrite` | `prepare_rewrite` |
| `vvv_review_plan`     | `review_plan`     |
| `vvv_inspect_plan`    | `inspect_plan`    |
| `vvv_apply_plan`      | `apply_plan`      |
| `vvv_discard_plan`    | `discard_plan`    |
| `vvv_validate_plan`   | `validate_plan`   |

Arguments are the command's generated `arguments` schema. Read-only tools add
`max_output_bytes` (default 16384, or 32768 for discovery; range 1024–1048576).
`vvv_apply_plan` and `vvv_validate_plan` do not accept that field. No `command`, call ID, workspace root,
or arbitrary command dispatch is accepted. Apply and validation have `readOnlyHint: false` and
`destructiveHint: true`; other tools have `readOnlyHint: true` and
`destructiveHint: false`. Validation has `openWorldHint: true`; other tools have `openWorldHint: false`.
Discovery still reports the full engine catalog; use `tools/list` as the MCP
allowlist.

Reads and validation run through `Call::execute_with_cancellation`; apply runs through
`Call::execute` without a cancellation handle or output budget. Queued applies
can be cancelled, but active applies finish and retain their terminal outcome.
Cancellation after apply starts cannot replace its receipt with a cancellation
error; after an interrupted response, inspect or retry the same handle. `structuredContent` contains the original vvv `Response<Answer>`
without an MCP request ID. A text block contains the same JSON for compatibility.
Engine failures set `isError: true` and retain typed recovery fields. Ambiguous and
unavailable navigation are successful results with `isError: false`. Unknown tools
and invalid argument shapes are JSON-RPC errors, not vvv failures. Navigation returns compact definition locations; use `vvv_context` for bounded
source excerpts. Oversized candidate sets still return `output_limit`.

Limits apply independently:

| Resource                                     | Limit                                                                         |
| -------------------------------------------- | ----------------------------------------------------------------------------- |
| Incoming SDK codec frame                     | 64 KiB                                                                        |
| JSON-encoded request ID                      | 256 bytes                                                                     |
| Admitted requests, including pending replies | 32                                                                            |
| Engine execution                             | One worker, eight queued calls                                                |
| Serialized engine result                     | Reads: 16 KiB default (discovery 32 KiB), 1 MiB maximum; apply: 1 MiB maximum |
| Outgoing JSON-RPC message                    | 8 MiB                                                                         |

The outgoing allowance reserves space for structured content, its escaped text
copy, envelopes, and IDs before execution. Oversized error payloads become a small
`output_limit` failure. JSON is never truncated. Oversized input closes the
session; malformed JSON produces a protocol error and ends the decoder stream.
Duplicate or oversized IDs receive errors without an ID; capacity errors use the
rejected request ID. A busy session can be retried after an outstanding call
finishes. Backpressure retains admitted request slots until responses are written.

Cancellation removes queued jobs immediately. Active read calls check cancellation
between source reads, search batches, navigation lookups, context phases, and
publication. Cancellation and publication share one synchronization point: a
cancelled call cannot publish a cursor, while a call that has already published
returns its completed result. Existing cursor checkpoints remain replayable.
Cancellation latency is the next cooperative checkpoint, not a wall-clock limit;
a single parser invocation, directory walk, or filesystem operation cannot be
forcibly interrupted. EOF cancels pending work, joins the engine worker, and drops
the session's retained query handles.

The adapter uses the [official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)
for message codecs, lifecycle, and dispatch, following the
[tool result](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
and [cancellation](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation)
contracts. The engine and core do not depend on MCP or an async runtime.

## `relationships`

A read-only, bounded relationship query anchored to the same `origin` and
`selection` contract as `resolve`:

```json
{
  "command": "relationships",
  "origin": {
    "kind": "position",
    "path": "src/worker.rs",
    "position": { "line": 11, "column": 7 }
  },
  "kind": "callers",
  "scope": { "paths": ["src"], "packages": [] },
  "budget": {
    "max_items": 64,
    "max_bytes": 16384,
    "max_files": 128,
    "max_lookups": 1024
  }
}
```

`kind` is `callers`, `callees`, or `references`. `scope` and `selection` default to
unrestricted. Scope filters candidate source files, not target resolution; it uses
search's path-prefix and owning-package semantics. Callees are restricted to call
sites whose nearest named callable is the selected declaration. Nested functions
and anonymous callable bodies are not attributed to the outer callable.

The result contains `snapshot`, `subject` (the complete compact resolution outcome),
`kind`, `scope`, `items`, and `coverage`. Unavailable or ambiguous subjects produce
no sites and retain their resolution reason or full selectable candidate set.

Each item has:

- `site`: an exact, versioned `SourceAnchor` for the identifier or callee expression.
- `start`: zero-based source position; `spelling`: the exact text at `site`.
- `caller`: the owning named function/method's `SymbolRef`, or null for non-call
  references, module-level calls, and anonymous callable bodies.
- `call`: syntactic `direct`, `member`, or `indirect`, or null for a non-call use.
  Syntactic classification does not itself prove a target.
- `outcome`: `confirmed` with `target` and `evidence`; `ambiguous` with all compact
  definition `candidates`; `unavailable` with navigation's `reason`; or `indirect`
  with a known `binding` or null. A local/parameter/value binding used as a callee
  does not prove the invoked function. In `references`, a confirmed target denotes
  the referenced binding, without asserting runtime invocation.

Incoming candidates use the subject's name and local import spellings whose bindings
resolve to it or include it among ambiguous candidates. Import probes and site
resolution both count toward `lookups`. Resolution confirms or rejects each site independently,
including renamed named imports and re-export chains. Resolved different targets
are excluded; ambiguous incoming sets are retained only when they contain the
subject, with the whole set intact. Unavailable and indirect incoming sites remain
possibilities, not confirmed edges. Import bindings that cannot resolve are counted in `unresolved_imports`; their
spellings are not added to the site scan. Wildcard aliases, renamed namespace
members, and assignment-based aliases are not exhaustively enumerated.

`coverage` reports `files_scanned`, `files_remaining`, `lookups`, `omitted_items`,
`unsupported_files`, `unresolved_imports`, `scan_complete`, and `stopped_by` (`files`, `lookups`, `items`,
`bytes`, or null). A file counts as scanned when entered; a work limit may stop
within it, so zero `files_remaining` alone does not establish completion.
`omitted_items` counts whole discovered sites removed to meet the byte budget,
not unseen relationships. `scan_complete` means the candidate scan finished
without limits or unsupported call-fact providers, not that a runtime call graph
is complete. `limitations` explicitly includes `receiver_types`, `indirect_targets`,
`macro_expansion`, `anonymous_callers`, `unsupported_bindings`, and
`unenumerated_aliases`, even when the scan completes.

Budgets default to 16,384 bytes, 64 items, 1,024 lookups, and 128 files. Byte bounds
are 1,024–1,048,576; item bounds 1–1,024; lookup bounds 1–16,384; file bounds 1–4,096.
The byte count covers the entire compact JSON result. An indivisible first site or
subject that cannot fit produces `output_limit`; ambiguous candidate sets are never
partially delivered. The `Call.max_output_bytes` limit also constrains this query's
byte budget. There are no relationship continuation handles; narrow `scope` or
increase budgets to repeat a query.

A fresh workspace snapshot captures source contents, manifests, and inventory and
is revalidated before publication. Observed edits fail with a stale error. Snapshot
capture and validation, parsing, and source loading are outside relationship work
counters; budgets do not impose execution deadlines or total-memory limits.
Cancellation uses the shared cooperative read cancellation contract. Ordering is
source path then call/identifier position, with repeated sites preserved rather
than collapsing multiple calls to one target.

The MCP mapping is `vvv_relationships` → `relationships`, with the same arguments,
result schema, and shared call budgeting as other read-only tools.
