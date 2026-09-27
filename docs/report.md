# Reports and views

The engine returns typed capability results or an `Execution` from
`Engine::run(Request)`. At an interface boundary, `Execution::into_answer()`
produces the wire `Answer`. Human interfaces compose that answer into a `Document`,
then use a `View` to produce display rows. JSON interfaces serialize the answer.

```text
Answer ──▶ Document::of(&answer) ──▶ Document
                                      │
                           View::present(document, options, width)
                                      │
                                 Presentation
                                      │
                        CLI styling or TUI drawing
```

## Ownership

[report/](../crates/vvv-engine/src/report/mod.rs) owns shared vocabulary:

| Module        | Responsibility                                                         |
| ------------- | ---------------------------------------------------------------------- |
| `document.rs` | `Document`, shared construction methods, and answer/failure delegation |
| `block.rs`    | `Block`, `Note`, and `ReferencePlan`: facts retained for views         |
| `row.rs`      | `Row` and `Source`: displayed lines with optional source coordinates   |
| `view.rs`     | `View`, `Options`, `Presentation`, and `Detailed`                      |
| `lines.rs`    | Shared builders for styled lines and source-bearing rows               |

Per-answer composition is implemented as methods on `Document` beside each
capability's request, answer, and execution. The
[command ownership index](../crates/vvv-engine/src/capabilities/mod.rs) identifies
these owners. Shared wire data and styled-line vocabulary live in `protocol/`.

The CLI's [HumanReporter](../crates/vvv/src/output/human/mod.rs) composes a
`Document`, presents it through `TerminalView`, and styles the rows with `Palette`.
`TerminalView` delegates layout to `Detailed` and appends CLI flag advice.
The [JSON reporter](../crates/vvv/src/output/json.rs) serializes the wire result.
The TUI's [Compact view](../crates/vvv-tui/src/view.rs) produces list rows;
[mode screens](../crates/vvv-tui/src/modes/mod.rs) draw those rows with `Painter`.
The [report overlay](../crates/vvv-tui/src/overlays/screen.rs) presents a shared
`Document` through `Compact` or `Detailed`.

## Document composition

`Document` contains two ordered sequences of `Block`: `body` for results and
`notes` for summaries, hints, and warnings. Composition is pure: it reads answer
data without filesystem access or interface state. `Document::of(&Answer)`
delegates to the corresponding composition method. File preview has no rendered
document; its answer supplies the TUI's syntax-colored source pane.

Blocks retain the facts needed by every view. For example:

- `Declarations` retains matches, including their source coordinates.
- `Outline` retains its owning `RelPath` and declaration tree.
- `DepGroups` retains the source file, imports, and package grouping information.
- `Verdicts` retains occurrences and an optional `ReferencePlan` containing
  file changes and mutation state.
- `Changes` and `Moved` retain mutation state and file changes; `Moved` also
  retains respellings and notices.

A composition can emit shared styled `Line` values for titles, counts, and
summaries. It must retain structured source-bearing facts when a view needs their
coordinates. It does not select column widths, colors, expanded verdicts, or
patch visibility. `Document::of` takes no presentation options, and composition
and shared line builders do not branch on `verbose` or `diff`.

`Document::error(&Failure)` produces an error line in the note stream. Interfaces
choose how to display `Failure.hint` and structured recovery details.

## View selection

`View::rows(&Block, Options, width)` lays out one block.
`View::present(&Document, Options, width)` collects both streams into a
`Presentation`. An interface can specialize this presentation boundary to append
advice. `Options.verbose` controls expansion and reach details;
`Options.diff` controls full patches. The same document can be presented with
different options without recomposition.

`Detailed` produces file headers, grouped verdicts, summaries, and patch rows.
`Compact` produces short list rows and delegates blocks it does not specialize to
`Detailed`. The trait also supports rows for individual occurrences, relations,
respellings, notices, and rewrite matches so TUI panels can display facts without
constructing a complete command report. Views consume those fact types; renderers
only style or draw the rows they receive.

The CLI maps `Presentation.body` to stdout and `Presentation.notes` to stderr.
The TUI assigns the rows to panels, status, or an overlay. These stream names
represent result and note semantics, independent of either interface.

## Source coordinates

A `Row` contains a styled `Line` and an optional `Source { path: RelPath, line:
u32 }`. Source lines are zero-based. `Row::at` retains an actionable source;
`Row::new` has none. Declarations, outlines, imports, explanations, references,
respellings, and notices preserve their reported source sites. Summaries,
separators, and suggested imports have no source.

Diff rows use structured hunk coordinates. Preview rows refer to old-side context
and removed lines; applied rows refer to new-side context and added lines, using
the moved destination where present. A line absent from that side is not
actionable, and diff headers carry no source. The TUI uses these sites to select
report rows and open an editor at the selected line.

## TUI integration

Each mode owns its state, transitions, and typed view under `modes/<name>/`.
`BoundScreen<V>` binds the view to layout and panel callbacks once; callbacks read
the mode's own data without inspecting `Model` or `Mode`. `Model::update` routes
actions, and `worker.rs` calls the engine.

Mutation previews supply occurrences, respellings, matches, and file changes to
their mode panels. The `v` action switches compact and detailed row layouts.
Detail panes show the selected file's diff, including the hunk containing a rewrite
match. After apply, the worker composes a `Document` and returns it in
`Event::Applied`. `Overlay::Report` selects rows with source coordinates;
`j`/`k` navigate them and `e` opens the selected site.

## Boundary rules

- Capability-owned `Document` methods compose answer facts; shared report types
  define the vocabulary.
- View options affect presentation only. Source sites survive both composition
  and row generation.
- Line builders are methods on data-bearing types. Free functions and
  namespace-only unit structs are not valid owners.
- CLI renderers style `Presentation` without inspecting command answers.
- `Document` and `Presentation` are never serialized. `--json` remains the
  `Answer` contract documented in [protocol.md](protocol.md).
