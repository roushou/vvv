# Remaining audit work

The behavior repairs, dispatch consolidation, report boundary changes, and engine
navigation batch are complete. These items remain; each needs its own approved
plan. Follow [architecture.md](architecture.md) and the ownership rules in
[AGENTS.md](../AGENTS.md). New behavior must live on the type that owns its data or
in a trait implementation, never in a free function or a namespace-only unit
struct.

## TUI modes and screen integration

Group each mode's state, transitions, and screen under `modes/<name>/`, keeping
`Model::update` as the application router and `worker.rs` as the engine boundary.
Bind a typed view once so its panels cannot be paired with an unrelated mode;
replace the remaining rendering free functions with methods on those views, and
the nested string editor with a data-bearing `TextInput`. The audit evidence is
[state for every mode in model.rs](../crates/vvv-tui/src/model.rs#L212),
[mode branches across update.rs](../crates/vvv-tui/src/update.rs#L348),
[Screen and Panel callbacks accepting any Model](../crates/vvv-tui/src/screen/mod.rs#L34),
[rename callbacks repeatedly inspecting Mode](../crates/vvv-tui/src/screen/rename.rs#L180),
[history's free layout and rendering helpers](../crates/vvv-tui/src/screen/history.rs#L86),
and [the nested edit function](../crates/vvv-tui/src/update.rs#L556). This is an
ownership and integration correction; preserve action/effect behavior, first-plan
arrival handling, key precedence, and every TUI snapshot.

## API path unification

Choose one public-path policy for capability types in a separate API commit, with
explicit compatibility decisions and compile-time checks. The navigation batch
preserved existing routes: query types remain available at both the crate root and
`protocol::`, while the six migrated rename/move types are root-only. Evidence is
[protocol's query re-exports](../crates/vvv-engine/src/protocol/mod.rs#L25),
[the root exports and negative API doctests](../crates/vvv-engine/src/lib.rs#L11),
and the deferred API note in [architecture.md](architecture.md). This is a public
surface inconsistency, not a demonstrated resolution failure; do not silently
remove routes during structural moves. Keep serialized requests and answers
independent of the Rust path policy.

## Structured error hints

Represent suggested recovery or next actions as structured data owned by the
error, with CLI syntax and TUI wording chosen by their display layers. Today
[EngineError::hint](../crates/vvv-engine/src/error.rs#L117) returns strings containing
`vvv search` and `--in`, and
[conversion to Failure](../crates/vvv-engine/src/error.rs#L138) puts those strings
on the shared wire. The
[serve test explicitly expects CLI syntax in a JSON hint](../crates/vvv/src/cli/commands/serve.rs#L90).
`Failure.hint` is an optional string, and the
[TUI worker reduces failures to messages](../crates/vvv-tui/src/worker.rs#L109), so
clients cannot act on the suggested operation through a typed contract. Plan the
wire compatibility and display changes explicitly, retaining the structured
recovery details already introduced for failed mutations.

## Distinct capability errors

Distinguish a missing Layout, a missing Surgery, and an unsupported operation so
clients receive the capability that is actually unavailable. The concrete audit
evidence is [Namespace::surgery](../crates/vvv-engine/src/graph/namespace.rs#L49): a
language that has a Layout but no Surgery returns `EngineError::NoLayout`, whose
[message and code](../crates/vvv-engine/src/error.rs#L62) incorrectly say paths
cannot be followed. Keep component errors with their owners and map them into
EngineError and Failure at the boundary. Add a fake-language test with a Layout
and no Surgery, and treat any new public variant or wire code as an explicit API
change rather than folding it into a navigation commit.

## Small ownership fixes

Finish the remaining small violations of the ownership rule without inventing
utility namespaces. [Match::locate](../crates/vvv-engine/src/protocol/search.rs#L179)
lives in the wire module but takes a `SourceFile` and derives source coordinates;
Candidate already owns the file and language needed for that construction.
[FileChange::all](../crates/vvv-engine/src/protocol/result.rs#L151) makes protocol
data depend on Plan and FilePreview; its construction belongs with the plan and
preview data it uses. [Namespace construction](../crates/vvv-engine/src/graph/namespace.rs#L28)
relies on Graph checking for a Layout and later re-fetches it with `expect`; make
that capability requirement explicit at construction, without claiming an observed
panic from the existing checked caller. Also remove the
[Builtins namespace-only struct](../crates/vvv/src/languages.rs#L10) by giving the
composition root actual registry state, and put the
[free serde default helper](../crates/vvv-core/src/import/mod.rs#L47) on ImportRef,
which owns the field's default. Test-helper free functions are not exempt from the
rule either; migrate touched fixtures to data-bearing fixture owners rather than
adding more helpers. Preserve wire shapes and behavior unless a separately
approved API change is required.

## Cross-file parent aliases

Extend resolution to private module bindings imported from a parent file, keeping
provenance, visibility, and cycle handling explicit. Same-file alias chains now
resolve to a fixed point, but a parent with `use crate::a as parent` and a child
with `use super::parent as local` can still leave `local::Foo` unresolved. This is a
confirmed limitation, documented in [guide.md](guide.md#L408), with the retained
ignored regression
[child_modules_follow_private_module_aliases_imported_from_their_parent](../crates/vvv/tests/corpus.rs#L1061)
and its [Rust fixture](../crates/vvv/tests/corpus/rust-resolution/src/parent_context/nested.rs).
The repair must make Fragment, references, deps, and explain agree on the correct
Rust target, without weakening references to match an under-resolved edge. Remove
the ignore only when that fixture passes; explain any corrected query snapshots.
