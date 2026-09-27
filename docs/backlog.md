# Remaining audit work

The behavior repairs, dispatch consolidation, report boundary changes, and engine
navigation batch and the TUI mode pass are complete. The open items below remain; each needs its own approved
plan. Follow [architecture.md](architecture.md) and the ownership rules in
[AGENTS.md](../AGENTS.md). New behavior must live on the type that owns its data or
in a trait implementation, never in a free function or a namespace-only unit
struct.

## TUI modes and screen integration — done

The pass addresses the audit's all-mode state and transition files, callbacks
accepting any `Model` and repeatedly inspecting `Mode`, and rendering free functions.
Each mode now owns its state, transitions, and typed screen under
[modes/](../crates/vvv-tui/src/modes/mod.rs); overlays own their data and views under
[overlays/](../crates/vvv-tui/src/overlays/mod.rs). Shared
[Screen and Panel](../crates/vvv-tui/src/screen/mod.rs) hold key, focus, and help
metadata; `BoundScreen<V>` binds one typed view to its panel callbacks.
[Model::update](../crates/vvv-tui/src/update.rs) routes application actions and
[worker.rs](../crates/vvv-tui/src/worker.rs) remains the engine boundary.
[TextInput](../crates/vvv-tui/src/input.rs) owns the borrowed text buffer edited by
name, destination, and template inputs. The existing action assertions and TUI
snapshots pass without snapshot updates; first-plan arrival and key precedence
remain unchanged.

## Free-function inventory

The TUI production entries are done: all 33 inventoried functions now belong to
views or the data they operate on. No namespace-only struct was introduced.

| Audited TUI functions                                                                           | Status and owner                                                              |
| ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Rename: `layout`, `draw_name`, `draw_unsure`, `draw_sure`, `draw_other`, `draw_detail`          | Done: `RenameView` in `modes/rename/screen.rs`                                |
| Move: `layout`, `draw_to`, `draw_respellings`, `draw_structural`, `draw_notices`, `draw_detail` | Done: `MoveView` in `modes/moves/screen.rs`                                   |
| Rewrite: `layout`, `draw_template`, `draw_matches`, `draw_detail`                               | Done: `RewriteView` in `modes/rewrite/screen.rs`                              |
| Search: `layout`, `draw_query`, `draw_results`, `draw_context`                                  | Done: `SearchView` in `modes/search/screen.rs`                                |
| History: `layout`, `draw_header`, `draw_entries`, `draw_files`, `header`, `entries`, `files`    | Done: `HistoryView` in `modes/history/screen.rs`                              |
| Overlays: `draw_menu`, `draw_confirm`, `draw_help`, `draw_report`                               | Done: `MenuBox`, `ConfirmBox`, `HelpBox`, `ReportBox` in `overlays/screen.rs` |
| Overlays: `full`                                                                                | Done: `Region::full`                                                          |
| Nested input: `edit`                                                                            | Done: `TextInput::edit`                                                       |

The touched test helpers `layers` and `render` also now belong to data-bearing
`Layers` and `FrameFixture`. What remains is the production serde default `yes`
([ImportRef](../crates/vvv-core/src/import/mod.rs#L47)), the namespace-only
[Builtins](../crates/vvv/src/languages.rs#L10), and existing test-helper free
functions. In the TUI, those remaining helpers are the engine-value factories in
[fixtures.rs](../crates/vvv-tui/src/fixtures.rs), and the model, source preview,
numbered lines, key/input, effect-generation, mode/plan, history-entry, report, and
anchored-state fixtures in [tests.rs](../crates/vvv-tui/src/tests.rs). They are not
exempt from the ownership rule; migrate them to owners of the fixture data in a
separate test-fixture cleanup, preserving assertions and snapshots. API paths,
hints, capability errors, the other small ownership fixes, and cross-file parent
aliases remain open below.

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
