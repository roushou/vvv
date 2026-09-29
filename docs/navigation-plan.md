# Navigation implementation plan

This document proposes a navigation capability; the interfaces below are not yet
implemented. The existing boundaries in [architecture.md](architecture.md) apply.
Code snippets describe the important contracts and omit supporting definitions and
serialization details.

## Intended experience

Navigation starts from an exact source occurrence and resolves its declaration.
The definition preview, explicit follow actions, CLI, and AI clients use the same
engine capability. The TUI owns selection, focus, scrolling, and navigation history.

For example, searching for `Engine` should let a developer:

1. Select an import, field type, function parameter, or return type and see the
   referenced `Engine` definition in the bottom-left pane.
2. Follow that definition to make it the active browsing location.
3. Select `Workspace` inside the definition and follow its declaration.
4. Go back to the same position in `Engine`, then back to the original search,
   restoring selection, focus, and scroll positions.

Moving between result rows updates the preview without adding history entries.
Only explicit navigation adds entries. If resolution has several candidates, the
user or AI client chooses one; the resolver does not silently choose by spelling.

The existing preview presentation remains: a focusable, scrollable pane occupying
its full allowed dimensions, a plain `definition` title, no line-number gutter,
and consistently dedented source. A variant previews its containing enum and
highlights the variant. Loading another selection retains the previous complete
frame until the new result is ready.

## Why a dedicated capability

The current `ReferencesQuery::definitions` collects reference evidence separately
for same-named declarations. That fixes collisions when previewing imports and
type uses, but performs more work than resolving one occurrence needs. The TUI
also participates in declaration selection, while definition resolution and file
loading happen as separate requests.

Introduce `NavigationQuery` so the engine owns occurrence resolution and returns
the definition text, highlights, and selection together. This removes independent
TUI name-resolution rules and prevents combining a target from one source version
with a preview from another.

## Source identity and requests

Names are search inputs; source locations are navigation inputs. Two `Engine`
tokens in `fn build(engine: Engine) -> Engine` have different anchors even when
they resolve to the same declaration. The parameter binding `engine` is a
different symbol.

```rust
pub struct SourceAnchor {
    pub path: RelPath,
    pub content: ContentId,
    pub span: Span,
}

pub struct SymbolRef {
    pub language: LanguageId,
    pub declaration: SourceAnchor,
    pub name_span: Span,
    pub kind: SymbolKind,
}

pub enum NavigationOrigin {
    Position {
        path: RelPath,
        position: Position,
        expected_content: Option<ContentId>,
    },
    Occurrence(SourceAnchor),
    Symbol(SymbolRef),
}

pub struct NavigationQuery {
    pub origin: NavigationOrigin,
}
```

`ContentId` identifies the complete source text, using the existing fingerprint
machinery through an appropriate public data type. `Span` remains a half-open
UTF-8 byte range. `Position` retains the existing zero-based line and character
column contract. All ranges must be in bounds and on character boundaries.

`SymbolRef` identifies a declaration in a particular source version; it does not
promise identity across edits or moves. Its declaration span covers the symbol's
extent, and its name span uses absolute coordinates in the same file.

Search results must expose the source version from which their anchors were
produced. Do not change `MatchId` into a logical symbol identifier: its existing
selection semantics remain separate. Candidate choices carry stable selection
IDs within the captured result and a versioned `SymbolRef` for subsequent requests.

Position requests support CLI and editor integrations that do not already have a
search result. An omitted expected content ID requests the currently observed
source. An occurrence or symbol request always validates its version before
interpreting its ranges.

## Outcomes and coherent previews

```rust
pub struct NavigationReply {
    pub snapshot: SnapshotId,
    pub outcome: NavigationOutcome,
}

pub enum NavigationOutcome {
    Resolved {
        target: SymbolRef,
        evidence: ResolutionEvidence,
        preview: DefinitionPreview,
    },
    Ambiguous {
        candidates: Vec<DefinitionCandidate>,
    },
    Unavailable {
        reason: UnavailableReason,
    },
}

pub struct DefinitionPreview {
    pub container: SymbolRef,
    pub source: SourceExcerpt,
    pub selection: Span,
    pub identifiers: Vec<SourceAnchor>,
}

pub enum UnavailableReason {
    NoIdentifier,
    Unresolved,
    Unsupported(UnsupportedConstruct),
    ExternalSourceUnavailable(ExternalSymbol),
}
```

The resolved target and the displayed container are distinct. Selecting an enum
variant targets that variant, while the preview container is the enum. Opening an
editor or starting a symbol operation must use the intended target, never silently
substitute its display container.

`SourceExcerpt` contains the source content ID, absolute byte range, text, and
highlights from the same captured source. The selection and identifier anchors use
absolute file coordinates; presentation translates them into excerpt coordinates
and screen columns. Dedenting text must preserve that mapping.

Identifier anchors let clients follow names inside the preview. Resolve them on
demand; do not recursively resolve every identifier while building a preview.
Large definitions may require bounded excerpt loading, with every range request
pinned to the same content ID. The UI must distinguish a partial excerpt from a
complete definition and permit scrolling to retrieve the remaining ranges.

`ResolutionEvidence` records the relevant binding and import/re-export path.
Candidates include their declaration location, kind, and evidence, ordered
deterministically. Candidates that remain possible must not disappear because a
different candidate has a more convenient name or location.

Unavailable outcomes are normal navigation results. Invalid requests, stale
anchors, I/O failures, and exhausted resolution budgets are typed errors or
explicit incomplete results, not `Unresolved`. Display code translates outcomes
into short messages such as “Select an identifier”, “Could not resolve this name”,
or “Source is outside this workspace”. External identity needs a separate type;
an absolute OS path must not be placed in `RelPath`.

## Engine ownership and resolution

Add `capabilities/navigation.rs` to own the request, answer, execution, and report
composition. Register `Request::Navigate` and `Answer::Navigate`, update the command
ownership index, and expose the query through the engine's public surface. Shared
client identity types belong in `protocol/`; plugin-facing facts belong in core.
No new crate or capability method on `Engine` is needed.

```rust
impl NavigationQuery {
    pub fn execute(self, engine: &Engine) -> Result<NavigationReply, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph)
    }

    pub(crate) fn execute_in(
        self,
        graph: &mut Graph,
    ) -> Result<NavigationReply, EngineError> {
        graph.navigate(self)
    }
}
```

The dispatcher calls the internal execution body under its existing operation
guard. It must not call the public method and acquire the guard again.

Resolution proceeds through the owning graph components:

1. Capture source and facts; validate the requested source version and range.
2. Identify the exact token and its syntactic context.
3. Resolve a declaration token to itself.
4. Inspect lexical bindings in the language's applicable namespace.
5. Follow imports, qualified paths, aliases, and re-exports.
6. Consult an optional semantic provider when syntax cannot decide.
7. Return a confirmed target, all remaining candidates, or a structured reason.
8. Build the preview from the target's captured source and facts.

Reuse the existing `Fragment`, `Scope`, and re-export machinery. Track visited
resolution nodes and impose work budgets so cycles terminate. Reaching a budget
does not prove that no definition exists.

For example:

```rust
use vvv_engine::Engine as Runtime;

struct App {
    engine: Runtime,
}
```

The field's `Runtime` token follows its imported binding and re-export origin to
`Engine`. It does not search the workspace for declarations called `Runtime`.

### Lexical scope is a correctness requirement

The current facts and module scope do not fully model local bindings, generic
parameters, or every language namespace. A unique workspace spelling is not proof
of a target:

```rust
fn run<Engine>(value: Engine) {}
```

Here the type use refers to the generic parameter. It must not navigate to an
unrelated module-level `Engine` struct.

Extend plugin facts with the binding, scope, and identifier-context data needed
for supported cases. The generic syntax adapter performs extraction;
language-specific modules contribute declarative rules. Keep parser nodes and
traversal inside `vvv-lang/src/syntax/`.

The first delivery must either resolve a construct correctly or identify it as
unsupported. Deeper lexical coverage can arrive later without allowing false
confirmation in the initial version. Receiver-dependent methods, inferred types,
and macro-generated declarations require explicit capability limits until a
suitable semantic provider exists.

## Snapshot and cache contract

`ContentId` describes one source text. `SnapshotId` identifies the captured
resolution inputs, including source dependencies, imports, project configuration,
and any semantic-provider state used by the answer. It is an opaque identity, not
a claim that the operating system froze the filesystem.

The engine operation lock serializes engine operations; editors can still change
files concurrently. Capture immutable source inputs and derive text, highlights,
links, and evidence from those inputs. If relevant concurrent changes are
observed, revalidate the read dependencies and retry within a bound, then return a
typed stale result if a coherent answer cannot be produced.

Begin with conservative invalidation on graph revision changes. Cache keys must
include the origin version and resolution state; the origin file's content alone
is insufficient because an imported module or manifest can change the answer.
Provider changes must invalidate affected results too. Later, dependency-specific
invalidation can reduce unnecessary work.

Honor the engine's documented retention policy and provide an explicit refresh
path. A retained answer describes its captured inputs, not guaranteed latest disk
contents. Stronger freshness checks must be deliberate rather than implied by a
snapshot identifier.

Measure cold and warm navigation separately. Warm cursor movement should reuse
facts and scope indexes, avoid collecting workspace-wide references for each
token, and bound cache memory. Benchmark alias chains, duplicate names, and large
files before selecting finer cache policies.

## TUI requests, rendering, and focus

The search mode replaces its independent name-resolution fallback with the
navigation capability. The worker remains the only caller of the engine. Actions,
events, mode transitions, and rendering remain pure.

Every preview request carries a ticket, origin, page identity, and workspace
revision. Successes and failures carry the same identity. A reply can settle only
the request that is still current, including after Back or Forward navigation.

The central transition is illustrated below; `accepts` checks the complete
request identity, and `PreviewFrame` can represent content or a settled message.

```rust
pub struct PreviewState {
    pub shown: Option<Arc<PreviewFrame>>,
    pub pending: Option<PreviewRequest>,
    pub scroll: usize,
}

impl PreviewState {
    pub fn received(&mut self, reply: PreviewReply) {
        let Some(pending) = &self.pending else {
            return;
        };
        if !pending.accepts(&reply) {
            return;
        }

        let same_container = self.shown.as_ref().is_some_and(|shown| {
            shown.container().is_some()
                && shown.container() == reply.frame.container()
        });
        self.pending = None;
        self.shown = Some(Arc::new(reply.frame));
        if !same_container {
            self.scroll = 0;
        }
        self.reveal_selection_if_needed();
    }
}
```

Container equality includes its content version. Preserve scroll when another
occurrence resolves to the same displayed definition. Moving between variants in
the same enum preserves scroll unless the selected variant needs to be revealed.
A different definition or source version resets or deliberately remaps scroll.

While pending, retain the entire previous frame and its scroll state. Do not
replace its text with “Loading…” or apply the new target's selection to old text.
If needed, pending feedback belongs in a stable status area. Actions on retained
content use that frame's anchors; explicit follow of a newly selected row resolves
that row independently.

The worker coalesces obsolete cursor-driven preview requests and deduplicates
identical reads. Explicit follow requests and mutation operations are separate and
must not be dropped by preview coalescing. In-flight work may finish; stale replies
are harmless because every transition checks request identity.

The pane keeps its full allowed geometry throughout pending, resolved, ambiguous,
and unavailable states. No shortcut numbers or repeated symbol names return to its
title. Shared screen metadata owns focus and help bindings; rendering callbacks
receive typed mode views.

## Following identifiers and navigation history

Start with an identifier picker for the focused source pane. It lists the
identifier anchors already supplied by the engine, with surrounding context to
distinguish repeated spellings. Following a choice requests navigation for that
exact occurrence. This provides useful traversal without first implementing an
editor-style text cursor.

An ambiguous response opens a candidate picker showing location and symbol kind.
Choosing a candidate applies to that navigation action; it does not install a
global name-resolution preference. Keyboard bindings should be assigned through
the existing screen metadata after checking conflicts.

```rust
pub struct NavigationTrail {
    pub back: Vec<NavigationEntry>,
    pub current: NavigationEntry,
    pub forward: Vec<NavigationEntry>,
}

pub struct NavigationEntry {
    pub page: BrowsePage,
    pub selection: SelectionAnchor,
    pub focus: BrowsePanel,
    pub context_scroll: usize,
    pub definition_scroll: usize,
}
```

`BrowsePage` holds typed search, definition, or reference state within the TUI mode
structure. Retain immutable result data through `Arc` rather than copying file
contents into every history entry. History contains no executable mutation plans.

Push history only after a successful explicit follow. Failed navigation and
cancelled candidate pickers leave history unchanged. Back and Forward restore
the query/page, selected occurrence, focus, and both scroll positions. A new
successful follow after Back clears the forward branch.

Bound retained history by entry count and memory. When source changes invalidate
an entry, restore it as stale and refresh or re-resolve before following its
anchors. Never reinterpret an old byte offset in new text silently.

## Semantic-provider integration

The current `Oracle::refers` returns one optional location without source versions,
multiple candidates, external-source identity, or cancellation. It cannot alone
provide the complete proposed contract.

Deliver syntax and import navigation first. Treat a versioned semantic-provider
contract as a deliberate compatibility change, with explicit source versions,
candidate outcomes, external locations, and cancellation behavior. Validate
provider targets before presenting them as confirmed declarations.

Any language-server adapter must convert negotiated position encodings against
the exact source version. Do not pass vvv character columns or UTF-8 byte spans
directly as protocol positions. Provider integration should have separate Unicode
and stale-document tests.

## Impact and compatibility

| Area                    | Proposed change                                          | Impact                                                        |
| ----------------------- | -------------------------------------------------------- | ------------------------------------------------------------- |
| Engine                  | Navigation capability and coherent preview answer        | One occurrence-resolution path for every client               |
| Graph                   | Direct occurrence lookup and versioned caches            | Less repeated reference scanning; explicit invalidation       |
| Core and languages      | Lexical bindings and identifier context                  | Plugin contract changes; grammar-specific extraction tests    |
| TUI                     | Ticketed previews, identifier/candidate pickers, history | Stable rendering and reversible browsing                      |
| Protocol and CLI        | Navigate request, source versions, typed outcomes        | AI clients can distinguish ambiguity, absence, and stale data |
| References and rename   | Gradually share validated resolver components            | Preserve existing selection and plan preconditions            |
| Documentation and tests | Guide, protocol, snapshots, corpus                       | Public behavior and wire contracts stay reviewable            |

Adding fields may be additive for JSON consumers that accept unknown fields, but
adding fields to public Rust structs can break struct literals. Adding enum
variants can break exhaustive matches. Changing `Oracle` affects implementors.
Choose serialization defaults and release compatibility deliberately; do not
describe all of these changes as automatically backward compatible.

Keep existing name-based commands available. Reusing resolution in mutation
commands is a separate reviewed behavior change: navigation must not silently
broaden rename scope or bypass `Selection`, fingerprints, or `Plan`.

Update the command ownership index and public exports with the capability. Update
[guide.md](guide.md) when commands or keys ship, [protocol.md](protocol.md) when
wire data changes, and [architecture.md](architecture.md) when the implemented
ownership and contracts change.

## Delivery sequence

1. **Occurrence resolution and coherent previews.** Add source identities, typed
   navigation requests/outcomes, conservative scope handling, snapshot validation,
   and direct graph lookup. Expose the capability to programmatic and CLI clients.
   Replace TUI definition resolution with ticketed navigation replies while
   preserving the existing pane geometry and loading behavior.
2. **Following and history.** Add identifier and candidate pickers, explicit follow,
   Back/Forward state, stale-entry handling, and bounded retention. Preserve the
   editor action's intended declaration target independently of preview containers.
3. **Broader language coverage.** Extend lexical facts and semantic-provider
   integration for constructs the initial resolver reports as unsupported. Refine
   cache invalidation only after measuring correctness and performance.

Each slice must be useful independently. Unsupported cases remain explicit;
later coverage must not be a prerequisite for truthful results in earlier slices.

## Verification and acceptance

Engine tests use the shared fake language and pass without default features.
Parser-dependent cases live in the language crate, with real command behavior
represented in the corpus. Cover:

- Import, field type, parameter, return type, alias, and re-export navigation with
  same-named declarations in other files and packages.
- Declaration self-resolution, enum variant targets, and whole-enum containers.
- Generic and local shadowing, type/value namespaces, and unsupported constructs
  that must not resolve by globally unique spelling.
- Re-export cycles, private-parent aliases, and explicit incomplete resolution.
- UTF-8 boundaries, non-ASCII identifiers, character positions, and excerpt-to-file
  coordinate mapping after dedenting.
- Source changes between selection, resolution, and response; imported-file and
  manifest changes even when the origin file stays unchanged.
- Out-of-order successes and failures, duplicate responses, and replies from pages
  that are no longer active.
- Stable pane dimensions, retained pending frames, same-container scroll, and
  revealing another variant without an unnecessary reset.
- Candidate selection, failed follow, Back/Forward restoration after edits, and
  clearing forward history only on successful new navigation.
- Cache and history memory bounds, cold/warm costs, and rapid cursor movement.

Exercise TUI transitions with actions/events, then review `TestBackend` snapshots.
Use a PTY smoke test on this repository's `Engine` imports and type occurrences,
including same-named fixture declarations, to check focus, scrolling, and visible
flicker under rapid selection changes.

For implementation changes, run the repository's full build, test, clippy,
documentation, formatting, and feature-matrix gates with warnings denied as
specified in `AGENTS.md`. Review human-output and corpus snapshot changes before
accepting them. For this proposal alone, Markdown formatting and diff checks are
sufficient.
