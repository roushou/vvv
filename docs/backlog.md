# Open technical work

These items describe unresolved contracts and ownership issues. Implementations
must follow [architecture.md](architecture.md) and [AGENTS.md](../AGENTS.md):
behavior belongs to the type that owns its data or to a trait implementation.

## API path unification

Capability types have inconsistent public paths. Query types are available at both
the crate root and `protocol::`, while `Rename`, `RenameIntent`, `Move`,
`MoveIntent`, `MoveSymbol`, and `MoveSymbolIntent` are root-only. The evidence is
[protocol's query re-exports](../crates/vvv-engine/src/protocol/mod.rs) and
[the root exports and negative API doctests](../crates/vvv-engine/src/lib.rs).
Define one public-path policy, with explicit compatibility decisions and
compile-time checks. Serialized requests and answers must remain independent of
Rust import paths.

## Structured error hints

[EngineError::hint](../crates/vvv-engine/src/error.rs) returns strings containing
CLI syntax such as `vvv search` and `--in`; conversion to `Failure` puts those
strings on the shared wire. The [serve tests](../crates/vvv/src/cli/commands/serve.rs)
assert this JSON hint, while the [TUI worker](../crates/vvv-tui/src/worker.rs)
reduces failures to messages. Represent suggested actions as structured data owned
by the error so clients can act on them, and let each interface select its wording.
Specify wire compatibility for `Failure.hint` and preserve `Failure.recovery`.

## Distinct capability errors

[Namespace::surgery](../crates/vvv-engine/src/graph/namespace.rs) returns
`EngineError::NoLayout` for a language with a Layout but no Surgery. Its
[message and code](../crates/vvv-engine/src/error.rs) describe unavailable path
resolution rather than unavailable editing. Distinguish missing Layout, missing
Surgery, and unsupported operations; keep component errors with their owners and
map them to `EngineError` and `Failure` at the boundary. Cover a language with
Layout but no Surgery using the shared Fake, and specify any public variant or
wire-code changes.

## Source and preview ownership

[Match::locate](../crates/vvv-engine/src/protocol/search.rs) takes a `SourceFile`
and derives coordinates inside a wire module; `Candidate` owns the file and
language needed for this construction.
[FileChange::all](../crates/vvv-engine/src/protocol/result.rs) depends on `Plan`
and `FilePreview`; its construction belongs with those lifecycle types.
[Namespace](../crates/vvv-engine/src/graph/namespace.rs) relies on Graph checking
for a Layout and then re-fetches it with `expect`. Its constructor should retain
the required capability explicitly. The checked caller prevents a demonstrated
panic, but the type does not encode that precondition. Preserve behavior and wire
shapes when transferring these responsibilities.

## Composition and default ownership

[Builtins](../crates/vvv/src/languages.rs) is a namespace-only unit struct. Give
the composition root registry state so registration methods operate on owned
data. The free serde default helper `yes` in
[ImportRef](../crates/vvv-core/src/import/mod.rs) belongs on the type that owns
the field's default. Neither correction needs a behavior or serialization change.

## Test fixture ownership

Test-helper free functions are subject to the ownership rule. The TUI's
[fixtures.rs](../crates/vvv-tui/src/fixtures.rs) contains engine-value factories,
and [tests.rs](../crates/vvv-tui/src/tests.rs) contains helpers for models,
previews, input, effects, plans, history entries, reports, and anchored states.
Other test suites also contain free fixture helpers. Give these helpers
owners that retain the fixture data, following `Layers` and `FrameFixture`, and
preserve assertions and snapshots. `#[test]` functions remain exempt.

## Navigation coverage

Extend navigation coverage for struct/match patterns, receiver-dependent methods,
inferred targets, Rust inline modules and qualified type paths, TypeScript local
hoisting, wildcard exports, arbitrary namespace-member expressions, and package/path
aliases. External/generated source previews need dedicated source-provider support.

Definition previews retain complete files for interactive browsing; the context
capability provides bounded excerpts. Measure before introducing dependency-specific
result caching or changing the interactive preview protocol.

## Context and agent interface coverage

Context incoming scans are bounded and same-spelling only. Alias-complete
references and structural test identification need dedicated evidence and protocol
contracts. Signature extraction for initialized variables/constants and other
unsupported declaration forms needs grammar-specific contracts. Relationship queries classify call
expressions and follow named imports, but receiver-dependent targets, wildcard and
namespace alias enumeration, anonymous caller identities, and resumable relationship
scans remain open.
Session output budgets do not impose execution deadlines. MCP cancellation is
cooperative; parser invocations and individual filesystem operations are not preempted. Measure whole-workspace validation latency for
paged queries on large repositories before narrowing their invalidation scope.

## Reviewed mutations

Retained session handles cover rename, rewrite, file/directory moves, and selected
symbol moves. Extend the same review/receipt contract to batches; validation needs
final path/content effects derived from ordered staged plans. Symbol moves refuse
nested declarations, competing module/name bindings, qualified/shadowed companion
targets, and cross-scope targets without a local owner. Broader support needs distinct
binding addresses and additional ownership evidence, without silent candidate filtering.
Measure full-input capture latency on large repositories; ignored inputs and ambient
tool/dependency versions need explicit coverage contracts before broadening validation
claims.
