# Schemas, continuation, and MCP implementation plan

This document proposes three additions, in implementation order. All new commands,
types, limits, and wire examples below are proposed contracts. The current contract
remains in [protocol.md](protocol.md); crate ownership follows
[architecture.md](architecture.md).

The objective is an end-to-end workflow: discover the API, find a symbol, retrieve
bounded context, and request more information without guessing request shapes or
combining different source versions. Code intelligence continues to use the
existing ast-grep-based engine. These steps require no language-server installation
or managed language-server process.

## Scope and dependencies

| Step | Deliverable                                                | Depends on               |
| ---- | ---------------------------------------------------------- | ------------------------ |
| 1    | Published request/result schemas and schema discovery      | Existing protocol types  |
| 2    | Versioned search/context pages and exact excerpt expansion | Step 1 for new contracts |
| 3    | Read-only MCP tools over the same engine capabilities      | Steps 1 and 2            |

Existing `search`, `context`, `navigate`, and `serve` requests keep their current
meaning. Add separate paged commands so existing callers do not receive a different
result shape or acquire a cursor lifecycle unexpectedly. The first MCP tool set
uses those bounded commands. TUI pagination, mutation tools, retained edit-plan
handles, HTTP transport, alias-complete references, and inferred call graphs are
outside this plan.

Every implementation slice must include its protocol/guide changes, relevant
schemas, and tests. This document describes the intended implementation; it is not
a completed-work log.

## 1. Published JSON Schemas

### User experience and contract

Discovery currently lists parameter names and budgets. Extend each command entry
with references to its argument, request, result, and response schemas. Clients can
fetch a particular schema through a read-only `schema` command, without scanning
the workspace or making network requests:

```json
{
  "command": "schema",
  "for_command": "context",
  "contract": "arguments"
}
```

`arguments` describes command-specific fields; `request` additionally requires the
`command` discriminator; `result` describes the successful payload; `response`
describes the success/error envelope for that command. Publish aggregate `Call`
and `Reply<Answer>` schemas separately for `serve`, including `id` and
`max_output_bytes`. A command's arguments do not include those call-envelope fields.

The schema command returns an ordinary vvv response whose result includes the
schema identifier and JSON Schema document. For example, this is the proposed
definition of one property, not the whole context schema:

```json
{
  "type": "integer",
  "minimum": 1024,
  "maximum": 1048576,
  "default": 16384,
  "description": "Maximum compact JSON bytes in the context result."
}
```

Publish JSON Schema Draft 2020-12 documents. Explicitly set the dialect rather than
depending on a library default. Keep three identities separate:

- The existing vvv envelope `schema` number describes wire compatibility.
- A schema document identifier identifies a particular command/contract and
  artifact revision. Use immutable, locally resolvable identifiers, such as
  `urn:vvv:schema:1:context:arguments:<digest>`.
- An MCP protocol version identifies the transport contract in step 3.

The digest covers canonical document content before adding the self identifier.
Keep checked-in artifacts under `docs/schemas/v1/` and expose the same documents
through the engine. Bundle definitions locally; consuming a schema must not require
fetching remote `$ref` targets. Discovery should carry identifiers, not embed every
schema in its otherwise small reply.

### Type ownership and generation

Use generated schemas from the existing Rust wire types, with explicit overrides
where their Serde representation or validated domain differs from a derive's
default. Schemars supports different serialization and deserialization contracts;
generate input schemas for deserialization and output schemas for serialization.
See the [Schemars contract documentation](https://docs.rs/schemars/latest/schemars/generate/enum.Contract.html).

Proposed ownership:

| Location                               | Responsibility                                             |
| -------------------------------------- | ---------------------------------------------------------- |
| `vvv-core` wire types                  | Optional schema implementations beside the owning types    |
| `vvv-engine/protocol/`                 | Shared schema identifiers, contract kind, command identity |
| `vvv-engine/capabilities/schema.rs`    | Schema query, owned schema catalog, report composition     |
| `vvv-engine/capabilities/discovery.rs` | Command metadata and schema references                     |
| `vvv/src/cli/commands/schema.rs`       | Parse schema arguments and dispatch a request              |
| `docs/schemas/v1/`                     | Deterministic generated contract artifacts                 |

Introduce one typed command identity with an exhaustive mapping from `Request`.
Replace the separately maintained command strings in discovery with descriptors
keyed by that identity. Each descriptor binds its argument and result types,
read-only status, and schemas. Avoid a second hand-written inventory in the MCP
adapter. New commands must update the command ownership index.

Use an optional `schema` Cargo feature in core and engine. The core feature adds
only pure schema metadata; it must not add parsing, I/O, runtime, or workspace
dependencies. The CLI's proposed `schemas` feature enables the engine feature and
belongs in the default CLI feature set. Minimal no-default-feature builds continue
to work; their discovery reports schema support as unavailable and omits schema
references. Register the schema command only when compiled in.

Schemars belongs in workspace dependencies because core and engine both use it.
Select and lock a release compatible with the workspace's Rust 1.90 requirement.
Keep any independent JSON Schema validator a test dependency in its consuming crate.
Do not move existing types between crates just to make schema derivation easier.

The type-level change is intentionally small:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SourceAnchor {
    pub path: RelPath,
    pub content: ContentId,
    pub span: Span,
}
```

The difficult work is auditing representation, particularly `RelPath`, transparent
IDs, flattened enums, `Selection`, nullable `via`, defaulted budgets, legacy
matches without `content`, and custom mutation-state serialization. An untagged
answer union can have overlapping shapes: use `anyOf` where appropriate rather
than claiming exactly one branch matches. Per-command result schemas remain the
preferred client contract.

Schema constraints must agree with execution. Consolidate budget bounds/defaults
so validation, discovery, schemas, and documentation do not invent different
numbers. Do not globally forbid unknown properties where existing Serde decoding
accepts them. Preserve strict objects such as `ContextBudget`. A schema's `default`
documents a default; Rust deserialization still applies it.

Schemas describe shape and expressible constraints, not every runtime guarantee.
For example, a valid source range shape does not prove the range is in bounds or
matches the current file. Keep those checks in the engine. The schema query's
schema-document payload is an object, avoiding recursive schema generation of the
entire catalog inside itself.

### Implementation slices

1. **Schema foundations.** Add feature-gated schema support and audit shared wire
   representations. Implement explicit input/output generation settings and common
   budget metadata. Test difficult representations before covering all commands.
2. **Command catalog and schema query.** Bind all existing commands to concrete
   argument/result schemas, integrate discovery, and add `Request`/`Answer` routing
   plus CLI reporting. Schema/discovery requests must perform zero workspace reads.
3. **Artifacts and compatibility gate.** Add an engine example or dedicated test
   utility that emits artifacts deterministically and a CI check comparing them to
   checked-in documents. Export through a tool, not by adding file writes to a
   read-only engine capability. Document the exact regeneration command.

### Validation and acceptance

- Validate real serialized answers, including corpus outputs and error envelopes,
  against the corresponding per-command schema using an independent validator.
- Exercise accepted requests and rejected shapes: missing origin, wrong numeric
  types, out-of-range budgets, unknown strict budget fields, enum tags, and
  selection variants. Also test valid shapes that correctly fail runtime checks.
- Check omitted input defaults separately from required output fields; do not
  derive both contracts from a single sample JSON document.
- Ensure every command descriptor has schemas and every registered command appears
  once. Test no-language and schema-disabled builds.
- Verify deterministic artifacts and review schema diffs for breaking changes.
  Additive fields follow the existing v1 compatibility policy; do not silently
  tighten old request semantics as part of generation.

Done when a client can discover and validate a context request and all its outcome
shapes offline, and CI detects divergence between Rust serialization and published
artifacts. The main impact is optional compile-time dependencies and a new maintained
public contract; there should be no source-resolution behavior change.

## 2. Versioned pagination and expansion

### User experience and wire surface

Add `search_page`, `context_page`, `continue`, and `expand` capabilities. Existing
unpaged commands retain their shapes and behavior. Start with protocol/typed-engine
access through long-lived `serve` sessions; do not add a one-shot CLI continuation
flag whose backing process disappears after returning the first page.

An initial search request:

```json
{
  "id": 1,
  "command": "search_page",
  "query": { "pattern": "Engine" },
  "page": { "max_items": 20, "max_bytes": 8192 }
}
```

Every page contains a stable snapshot identity, operation kind, typed items,
operation-specific metadata, and an optional `next_cursor`. Continue in the same
session:

```json
{
  "id": 2,
  "command": "continue",
  "cursor": "opaque-query-cursor",
  "page": { "max_items": 20, "max_bytes": 8192 }
}
```

The cursor fixes the query, selection, ordering, snapshot, and traversal position.
Only delivery/work budgets may change on continuation; callers cannot swap a query
or workspace underneath the token. Use an explicit operation discriminator on
continuation results so clients can choose the appropriate schema.

An excerpt token means “more bytes from this declaration,” independently of the
query cursor meaning “more items/relationships”:

```json
{
  "command": "expand",
  "cursor": "opaque-excerpt-cursor",
  "max_bytes": 4096
}
```

The first expansion contract returns remaining exact source from the declaration
extent. Signature-only extraction is deferred until language facts describe
signature/body boundaries; do not infer a signature by cutting at the first brace.

### Page and source invariants

Introduce small shared wire types; execution stays with capability owners. This
sketch omits Serde attributes and operation-specific metadata:

```rust
pub struct PageBudget {
    pub max_items: usize,
    pub max_bytes: usize,
}

pub struct Page<T> {
    pub snapshot: SnapshotId,
    pub items: Vec<T>,
    pub next_cursor: Option<Cursor>,
}

pub struct PagedContextItem {
    pub item: ContextItem,
    pub expansion: Option<Cursor>,
}
```

Proposed delivery defaults are 20 items and 16,384 result bytes; maximums are 64
items and 1,048,576 bytes, with a 1,024-byte minimum. Publish these separately from
the existing context defaults. `max_items` is an upper bound, not a promise to fill
the page. Apply the smaller of page bytes and `Call.max_output_bytes` before
generating the page. Count the entire serialized result, including cursors,
metadata, escaping, and omission counters. Never cut a JSON document after encoding.

Search pages preserve the existing declarations-first, path/source order and
complete `Match` payloads. Include absolute one-based ordinals so page two does not
renumber matches from one. Stable `MatchId`s remain the preferred selection across
requests. Do not broaden mutation selection or treat a page-local ordinal as an
existing whole-query ordinal. Retain skipped-language diagnostics in page metadata.

If one complete search item cannot fit, return a structured `output_limit` with the
minimum required size and, when available, its source anchor. Do not skip it, emit
an endlessly empty page, or return a partial `Match` with misleading captures.
Increasing the budget or narrowing the query is required; context/expansion is the
alternative when the client already has a source target.

Context preserves seed, enclosing declaration, outgoing occurrence, and incoming
path/source order. Deduplication retains its existing first-evidence rule across
pages. A paged item can contain an exact partial excerpt and an expansion token.
An expansion response records the full requested extent, the returned anchored
range, its starting position, exact text, and a possible next excerpt token.
Chunks must concatenate to precisely the captured extent, with no skipped bytes,
synthetic ellipses, dedentation, or broken UTF-8. Use a distinct `done` flag for
the last chunk; an existing context item's `complete` continues to mean it contains
the entire declaration, not merely the last chunk.

Ambiguous navigation candidates remain complete. Do not paginate or truncate the
candidate set into something that looks uniquely resolved. If it cannot fit,
return `output_limit`. Unavailable targets retain typed reasons.

### Snapshot validity

Existing navigation snapshots are not sufficient as continuation state: a later
page can depend on files that earlier lookups never consulted. Introduce a private
`QuerySnapshot` that captures the full input universe relevant to the query:

- Sorted eligible source-file inventory and complete content identities, including
  files with no current match, because an edit can introduce a match.
- Project manifests and workspace walk/configuration inputs affecting inclusion
  and resolution, plus the registered language identities.
- The normalized query, selection, and ordering contract.

Start conservatively with identities for all registered source files and resolution
inputs for paged queries. More selective invalidation can follow measured evidence.
Keep the inventory and hashes, retaining full text only where a traversal or excerpt
needs it. Later loads must match the original recorded identity before use. Share
captured source allocations with the graph where possible; account for allocations
retained by a query even when another component originally loaded them.

Capture and validate through `Workspace`/`Vfs`, with a fresh file inventory and
content checks. Bypass `Retention::Session`'s trusted walk when establishing or
validating a continuation. Mtime/size alone is insufficient. Include additions,
deletions, renames, manifest edits, and changes that affect previously unmatched
files. Initial collection and each continuation revalidate before publishing their
answer. Work must use captured source versions, never silently refresh only part
of a retained traversal.

A detected change returns the existing `stale` failure with structured recovery
advising a fresh query. Invalidate all query/excerpt tokens rooted in that session.
An internal apply/undo must invalidate retained queries as well, including failed
operations that could have attempted writes. External changes still require
validation. This detects observed changes; it does not lock editors or guarantee
filesystem transaction isolation.

Full validation is an explicit performance cost: pagination bounds delivery and
retained state, but it does not make whole-workspace validation constant-time.
Measure initial-page and continuation latency on large repositories before changing
the invalidation strategy.

### Retention, retry, and resource limits

Own retained queries in a private engine `QueryStore`, shared by clones of the same
engine. It holds typed `SearchSession`, `ContextSession`, and excerpt state, not
arbitrary serialized `Answer` values. The CLI and MCP adapter never own resolution
state or cursor interpretation. Add capability-owned typed execution methods and
`Request` routing; do not add public pagination methods on `Engine`.

Keep `SearchPageQuery` with search in `capabilities/search.rs`, `ContextPageQuery`
and `ContextSession` with context in `capabilities/context.rs`, and source expansion
in `capabilities/excerpts.rs`. A `capabilities/pagination.rs` owner holds shared
page contracts and continuation dispatch over the closed set of retained query
kinds. Put `QueryStore` and its methods in `query_store.rs`, and source-validation
state in `graph/query_snapshot.rs`. Shared opaque cursor identity belongs in
`protocol/`. These are modules within the existing engine, not new crates or a
generic plugin framework.

Keep operation exclusion and an explicit lock order: operation guard, graph, then
query store if both are needed. Prefer extracting an immutable checkpoint and
releasing the store lock before expensive graph work, then publishing under the
same operation guard. Avoid recursively acquiring the guard through `Engine::run`.

Tokens are opaque, process-local handles with a store-instance identity, cursor
kind, query generation, and checkpoint identity. Validate all components; a token
from another process must never accidentally select a query with the same counter.
They are not persistent bookmarks or authentication credentials. `continue` rejects
excerpt tokens and `expand` rejects query tokens. Restarting `serve` invalidates
all handles, including on the same workspace.

Proposed initial retention policy: 16 active queries, 32 MiB per query, 128 MiB
total, and a fixed ten-minute lifetime. Account for snapshots, results, traversal
checkpoints, tokens, and any replay cache. Use monotonic time; make expiry tests
deterministic by passing instants into store methods. Admission fails explicitly
with `retention_limit` if a query cannot fit. Evict least recently used complete
query trees, including excerpt handles, when admitting another query; never evict
the currently executing checkpoint mid-response. Discovery publishes these limits.

Cursor use is retryable rather than consuming: the same token with the same budget
returns the same page and successor while retained and valid. A different budget
starts at the same logical position and can produce a different successor. Keep
checkpoints immutable; sharing state must not duplicate the entire workspace per
page. Bound checkpoint growth under the same retention policy. Never advance a
cursor on an output-limit error, cancellation, or failed validation.

Use typed failures with structured recovery data:

| Failure           | Recovery                                                            |
| ----------------- | ------------------------------------------------------------------- |
| `stale`           | Restart from the original query/current source position             |
| `cursor_expired`  | Restart; state was evicted, expired, or belonged to another process |
| `invalid_cursor`  | Correct malformed token or wrong cursor kind                        |
| `output_limit`    | Increase delivery budget or narrow the request                      |
| `retention_limit` | Narrow the query or restart with smaller retained scope             |

Keep messages in the existing display/error layer. Do not require clients to parse
prose to decide whether restarting or increasing a budget is appropriate.

### Resuming context work

Refactor context assembly into a capability-owned `ContextSession` with explicit
phase, next outgoing occurrence, next incoming file/token, seen-target set,
observed versions, and pending item. The existing one-shot `ContextQuery` should
reuse the same assembly rules while preserving its present budgets and output.

Separate delivery budgets from work budgets. For paged context, `max_lookups` and
`max_files` limit additional work in that call; resuming continues the frontier
instead of repeating earlier lookups. The seed resolution occurs once. Capture
remaining work as a cursor, and report per-call work usage and whether traversal
is complete. An empty page is allowed only when work progressed without yielding
an item and a successor exists; it must not repeat the identical frontier.

Do not redefine existing `ContextOmissions` counters. Paged context needs distinct
deferred-work metadata: stopping for a budget is resumable, while an ambiguous or
unsupported relationship may remain unavailable. No total relationship count is
known until traversal is exhausted. Incoming evidence remains same-spelling only,
and test-path evidence remains weaker than coverage.

`context_page` takes `origin`, `selection`, `references`, `page`, and `work`.
`work` contains `max_lookups` and `max_files`, defaulting to 64 each and retaining
their current maximums of 512 and 1,024. Context continuations can also supply
`work`; search continuations reject that option rather than silently ignoring it.
Snapshot validation is additional work outside those relationship-traversal counts.

### Implementation slices

1. **Store and snapshot foundation.** Add cursor/budget/recovery wire types,
   snapshot capture and validation, bounded retention, expiry, and deterministic
   checkpoints. Test against the shared fake language and memory Vfs.
2. **Search pages.** Add `search_page` and search continuation. Initially retain
   the existing ordered search result with explicit admission limits. Stop
   collection when retained-result limits are exceeded instead of collecting an
   unbounded vector and checking only afterwards. Preserve existing unpaged search.
3. **Exact excerpt expansion.** Add source-chunk fitting and excerpt handles tied
   to retained snapshots. Test reconstruction and retries before linking it into
   context results.
4. **Context pages.** Introduce the resumable context owner, expose `context_page`,
   and return expansion handles for clipped items. Preserve one-shot context
   through corpus comparisons; never accumulate one unbounded full context result.
5. **Session integration and contract publication.** Integrate call budgets,
   discovery, schemas, reports, guide examples, and real multi-request `serve`
   tests. Initial pages and continuations must use the same engine path for Rust
   callers and protocol clients.

### Validation and acceptance

- Concatenated search pages equal the existing whole result, including order,
  absolute ordinals, IDs, diagnostics, and no gaps or duplicates.
- Context across small work/delivery budgets agrees with a sufficiently budgeted
  traversal within the same supported scope. Deduplication and first evidence hold
  across phase boundaries and retries.
- Expansion reconstructs exact source for ASCII, multibyte Unicode, CRLF, quotes,
  backslashes, and declarations much larger than one response.
- Budget tests count serialized bytes including metadata and tokens. Oversized
  indivisible items and candidate sets fail without advancing the checkpoint.
- Change unmatched files, add/remove files, alter manifests, and modify a source
  while preserving its timestamp/size. Continuations must reject observed changes.
- Exercise replay, different delivery budgets, expiry, eviction, wrong cursor kind,
  cross-engine/process tokens, store limits, and concurrent calls through clones.
- Measure retained memory and continuation latency; distinguish graph/parser
  memory from the new store's enforceable limits.

Done when a client can retrieve search pages and a long definition in bounded
pieces, retry a request safely, and recover explicitly after an edit or expiration.
The main tradeoff is retained memory plus conservative source validation; no claim
is made that a byte budget bounds parsing time or total engine memory.

## 3. Thin MCP adapter

### Tool surface and transport

Add `vvv -C /path/to/repo mcp`, serving a fixed workspace over stdio. Keep
`vvv serve` as its existing JSON-lines interface. The adapter calls the engine in
process; it does not launch another vvv process or parse terminal output.

Use the official Rust MCP SDK for lifecycle, negotiation, framing, and tool
dispatch, with its stdio transport and only required features. Pin an MSRV-compatible
released dependency. Do not implement JSON-RPC framing or protocol negotiation by
hand. [Official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)

Initial proposed tools:

| Tool           | Engine request | Purpose                                            |
| -------------- | -------------- | -------------------------------------------------- |
| `vvv_discover` | `Discover`     | Languages, limits, and available capabilities      |
| `vvv_search`   | `SearchPage`   | Bounded symbol/occurrence discovery                |
| `vvv_navigate` | `Navigate`     | Resolve an exact occurrence with explicit outcomes |
| `vvv_context`  | `ContextPage`  | Bounded context with resumable relationship work   |
| `vvv_continue` | `Continue`     | More search/context items                          |
| `vvv_expand`   | `Expand`       | More bytes of an exact declaration excerpt         |

Use an explicit read-only allowlist. Do not automatically expose every command or
provide a generic “run arbitrary vvv request” tool. Navigation currently contains
full preview source: enforce a result budget and return `output_limit` with advice
to use context if it exceeds that budget. Adding a compact navigation representation
would be a separate engine contract, not an adapter-side truncation.

MCP tool definitions carry input schemas and may carry output schemas. Return
structured results conforming to those schemas. Generate definitions from step 1's
catalog and the small explicit tool-to-command mapping. Tool-list pagination is
separate from pagination inside search/context results. See the
[MCP tools specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).

Example invocation, using step 2's proposed context-page arguments:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "tools/call",
  "params": {
    "name": "vvv_context",
    "arguments": {
      "origin": {
        "kind": "position",
        "path": "src/engine.rs",
        "position": { "line": 19, "column": 11 }
      },
      "page": { "max_items": 12, "max_bytes": 8192 }
    }
  }
}
```

### Ownership and dependency boundaries

Keep the adapter in `crates/vvv/src/mcp/`, with the clap command in
`cli/commands/mcp.rs` and output/error conversion in `output/mcp.rs`. No new crate
is needed: there is one consuming binary, and optional dependencies can isolate
the MCP runtime. The interface imports engine types only; `languages.rs` remains
the sole plugin composition root.

Add an opt-in CLI `mcp` feature that enables `schemas`, the MCP SDK, and the runtime
features actually required. Keep it outside the default CLI feature set initially;
enable it explicitly for distributed release binaries and document
`cargo install vvv-rs --features mcp` for source installations. Update the release
workflow's binary build features and verify all release targets. Engine and core
must not acquire MCP or async-runtime dependencies. A dependency used only by the
binary belongs in that crate's manifest, not workspace dependencies.

The adapter owns initialization, tool naming, transport state, request scheduling,
and protocol error mapping. Engine capabilities own source access, query handles,
selection, budgets, stale checks, and recovery data. Tool descriptions and human
messages belong to the display layer. Data crossing the boundary remains typed.

The dispatch boundary is deliberately small; this is a design sketch rather than
SDK-specific compilable code:

```rust
impl McpSession {
    fn execute(&self, request: Request, max_output_bytes: usize) -> Reply<Answer> {
        Call {
            id: None,
            max_output_bytes: Some(max_output_bytes),
            request,
        }
        .execute(&self.engine)
    }
}
```

Keep output policy on this shared path. Calling `Engine::run` directly and fitting
the result afterwards would bypass context/page budget integration. All exposed
tools have a default output budget even when their arguments omit one.

### Results, limits, and errors

Return the existing per-command vvv `Response` envelope as MCP structured content,
without copying MCP's request ID into it. Thus output schemas describe both a
successful result and a typed vvv failure. Preserve `schema`, outcome, source
anchors, evidence, omissions, and continuation handles exactly.

Map malformed MCP requests, unknown tools, and invalid argument shapes to protocol
errors according to the negotiated protocol/SDK. Map engine execution failures
such as stale sources and expired cursors to tool results with `isError: true` and
structured vvv recovery data. Ambiguous and unavailable navigation outcomes remain
successful data; the client must inspect the outcome instead of assuming one target.

For the supported 2025-11-25 baseline, include serialized structured content in a
text content block for compatibility, as the specification recommends. Account for
this duplication explicitly: `max_output_bytes` still measures the engine result,
not total MCP framing. Set a separate server transport-message cap and reserve
space for both representations, JSON escaping, envelopes, and bounded request IDs
before executing. If serialization still cannot fit, return a small typed error
without consuming a continuation checkpoint. Never truncate structured JSON.

Only protocol messages go to stdout; diagnostics go to stderr. Fix the workspace
root at launch and preserve `RelPath` validation at the engine boundary. Bound
incoming frames, queued calls, and outgoing messages separately from result budgets.
Do not accept arbitrary workspace changes from tool arguments. See the
[MCP stdio transport contract](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports).

### Scheduling and cancellation

Use one bounded execution queue per MCP session, matching the engine's operation
serialization. Run synchronous engine work on a dedicated worker or bounded
blocking task, leaving the transport event loop able to read cancellation and EOF.
Do not spawn an unlimited blocking task for every incoming request.

Queued calls can be removed when cancelled. Extend shared read-only execution with
a cooperative cancellation context for in-flight work, checked between file reads,
search batches, navigation lookups, context phases, and result publication. The
existing navigation-provider token is useful precedent, but it does not currently
make ordinary syntax search/context cancellable. Keep a compatibility path for
`Call::execute` that supplies a non-cancelled context.

A cancelled call must not advance or publish a continuation checkpoint. Dropping
an async future does not by itself stop synchronous parsing. Document the remaining
latency bound as the next cooperative checkpoint; a single parser invocation is
not forcibly interrupted. On EOF, cancel queued/in-flight work, release query
handles, and shut down without retaining a background session indefinitely. See the
[MCP cancellation contract](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation).

Negotiate protocol versions through the SDK. Test a named supported baseline
(2025-11-25) and any additional version the pinned SDK advertises. Check its exact
tool-result and cancellation contracts before enabling another version; do not
declare support merely because initialization accepts a version string.

### Implementation slices

1. **Transport and feature isolation.** Add the optional SDK/runtime dependencies,
   `mcp` command, initialization, fixed root, stdout discipline, and clean shutdown.
   Validate the SDK's MSRV and release-target compatibility before wiring tools.
2. **Catalog-backed read-only tools.** Expose the six mappings above with generated
   schemas, typed decoding, shared call budgets, structured replies, and output
   conversion. Keep discovery accurate for the languages/features actually built.
3. **Cancellation and bounded scheduling.** Add the execution queue and cooperative
   read-only cancellation path. Test cancellation without races in cursor state or
   a blocked transport reader. Bound request and response messages.
4. **Client integration and distribution.** Exercise a real SDK client over stdio,
   publish launch examples, enable the release feature, and add feature-matrix CI.
   Do not change default `serve` framing or make MCP dependencies mandatory for
   engine consumers.

### Validation and acceptance

- A real client initializes, lists tools, validates their schemas, searches, follows
  a result, requests context, continues, expands, and closes the connection.
- Feed equivalent requests through typed engine execution, `serve`, and MCP;
  compare semantic payloads, normalizing only opaque handles and transport fields.
- Validate every structured tool response against its advertised output schema,
  including stale, ambiguous, unavailable, budget, and expired-cursor cases.
- Send bad tool names/arguments, oversized frames, and many queued calls; verify
  bounded scheduling, correctly classified errors, and no non-protocol stdout.
- Cancel while queued, during traversal, and just before publishing a page. Retry
  the cursor and verify that no results disappeared or duplicated.
- Verify engine/core builds without schema or MCP features, CLI `mcp` with no
  languages, `mcp,rust`, `mcp,typescript`, and all-features builds.

Done when a compatible AI client can complete the bounded navigation/context
workflow using discovered tools, with no custom vvv transport code and no extra
code-intelligence process. The main impact is optional binary/runtime dependencies
and session lifecycle management; resolution remains owned by the engine.

## Shared verification and delivery

Before declaring an implementation slice complete, run the relevant tests and the
repository gates required by [AGENTS.md](../AGENTS.md), with warnings denied. The
full gate includes workspace/all-feature tests and corpus, build, clippy, rustfmt,
dprint, rustdoc, the existing language/no-language feature matrix, and a real CLI
dogfood query. Extend the matrix for the schema and MCP features above.

Engine tests use `tests/common/mod.rs`'s fake language and `MemoryVfs`; grammar
coverage stays in the language crate and corpus. Compare paths by component and
normalize embedded diagnostic path separators in cross-platform assertions. Review
changed corpus/human snapshots before acceptance. New commands need report
composition, command ownership entries, protocol documentation, and guide examples
in the same slice.

For this documentation-only plan, validation is Markdown formatting, local link
resolution, and consistency with the current protocol and crate boundaries. It does
not require rerunning Rust tests. Implementation and commits require separate user
instructions.
