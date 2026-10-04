# Testing

Pick the layer that owns the behavior:

| Question                                 | Test location                                           | Assertion                                                |
| ---------------------------------------- | ------------------------------------------------------- | -------------------------------------------------------- |
| Can plugin data be consumed safely?      | `vvv-core/src/` tests, `vvv-engine/tests/primitives.rs` | Typed errors and valid round trips                       |
| Does an engine capability work?          | `vvv-engine/tests/`                                     | `Fake` language, focused facts and complete source trees |
| Does a language understand this syntax?  | `vvv-lang/src/<language>/tests/`                        | Real grammar and edits                                   |
| What does a command mean on real code?   | `vvv/tests/corpus.rs`                                   | Per-command human/JSON snapshots and mutation properties |
| Does a key/event change the right state? | `vvv-tui/src/tests_<concern>.rs`                        | Pure `Action`/`Event` assertions                         |
| What does an interface display?          | CLI human snapshots, TUI presentation tests             | Reviewed `insta` snapshots                               |

Test the current implementation as the first release. Previous formats and
migrations have no test contract. A test earns its place by protecting a distinct
behavior, invariant, failure outcome, or interface contract:

- Keep syntax details in language tests and the corpus. MCP tests cover protocol
  negotiation, schema-valid responses, budgets, cancellation, and client workflows.
- Keep source provenance, UTF-8 boundaries, ambiguity, rollback, and mutation laws.
- Remove accessor checks, fixture demonstrations, and weaker duplicates when a
  stronger test already exercises the same behavior.
- Optional inputs and external protocol negotiation are current contracts;
  accepting records from a previous vvv implementation is not.

Ignored subprocess fixtures are invoked by validation workflow tests. Ignored
performance workloads are manual diagnostics. Neither is an unfinished test.

## Focused runs

```console
# Full gate with summaries instead of every passing test name.
cargo test --workspace --all-features --no-fail-fast --quiet

# List individual corpus commands; each runs human and JSON output.
cargo test -p vvv-rs --test corpus -- --list
cargo test -p vvv-rs --test corpus rust_golden::case_rename -- --exact

# Mutation guarantees and malformed plugin evidence.
cargo test -p vvv-rs --test corpus apply_is_preview
cargo test -p vvv-rs --test corpus batch_is_composition
cargo test -p vvv-engine --no-default-features --test primitives

# Review state and executable-plan ownership.
cargo test -p vvv-tui review
cargo test -p vvv-tui worker::tests
```

Use `tests/common/fixture.rs`'s `EngineFixture` for a parser-free engine and observable
storage. Add behavior to the shared `Fake` instead of defining another language.
Use `$0` when a test needs a cursor; `EngineFixture::marked` removes it and computes
the position, including Unicode and preceding lines:

```rust
let fixture = EngineFixture::marked(&[
    ("package", "ws"),
    ("a.p", "def Engine"),
    ("use.p", "use a.p/Engine\n$0Engine"),
]);
let reply = NavigationQuery::at("use.p", fixture.cursor("use.p"))
    .execute(&fixture.engine)
    .unwrap();
```

`source_tree()` compares every source path and its contents, excluding engine history.
Use `FaultFixture` for injected failures; its `source_tree()` reads backing storage
without consuming faults. Assert history separately when it belongs to the guarantee.

The corpus enforces three mutation laws: applying the captured plan equals the
complete previewed tree; undo restores the original tree; declared batch pairs equal
sequential application. Every declared pair must succeed. Keep an independent edit
oracle in these tests so production edit application cannot validate itself.
Each command case is registered with `corpus_cases!`; its manifest test rejects
missing or duplicate cases so additions cannot silently go untested.

Snapshots review presentation and command meaning. State transitions, selection,
provenance, and rollback use focused assertions. Compare paths as `Path` values,
never formatted separators. Review snapshot diffs before accepting them.

Corpus human snapshots preserve exact text. JSON snapshots preserve every field,
value, and array position, with sorted object keys and compact small records.
Navigation cases reference a `__source__` snapshot containing the complete file
preview: text, highlights, symbols, and identifiers. Each fixture/path has one such
expectation; every response's full preview must equal it. Cases must also agree
with each other when accepting updates, so accepting snapshots cannot conceal
query-dependent source differences. `$snapshot` references exist only in tests;
the actual CLI response is validated against its schema before factoring.

These patterns use [Cargo's native test filtering](https://doc.rust-lang.org/cargo/commands/cargo-test.html),
[`$0` fixture positions used by rust-analyzer](https://github.com/rust-lang/rust-analyzer/blob/master/crates/test-fixture/src/lib.rs),
and [Serde's checked `try_from` deserialization](https://serde.rs/container-attrs.html#serdetry_from--fromtype).
The shared fixtures are test support modules and require no production crate or new dependency.
