# The report and its views

A command's result is data. `vvv-engine::report` composes it into a `Document`
— the report — and an interface lays that document out through a `View`. The
CLI and the picker share the document and the seam; each holds the views it
draws.

## The shape

```
Engine::run ──▶ Answer                  the wire result (what --json prints)
                  │
      Document::of(a, options) ──▶ Document   the report: facts, no layout
                                       │
                                    View ──▶ Presentation   the rows to draw
                                       │
                             ┌─────────┴─────────┐
                           CLI render/         TUI render/
                         (ANSI to stdout)   (ratatui Buffer)
```

- **`Answer`** — the engine's per-command result; the JSON contract. It does
  not change.
- **`Document`** — the report as data: a `Vec<Block>` of facts per stream. No
  line a command chose, no colour, no width. `Document::of` builds it.
- **`View`** — how a document is shown: a document plus the room it has and the
  flags it was asked for, rows out. `Detailed` is the terminal's, `Compact` the
  picker's.
- **`Presentation`** — the rows, each with the source it stands for, split into
  the result stream and the note stream.

## Where things live

```
crates/vvv-engine/src/
  protocol/                 the wire: Answer, its parts, display::Line, vocabulary
  report/
    mod.rs                  Document, Block, Row, Site, Note, Options, the composition
    lines.rs                the row builders over protocol data
    view.rs                 the View trait, Presentation, the Detailed view
crates/vvv/src/output/
  mod.rs                    Reporter, OutputFormat
  human/mod.rs              HumanReporter: Renderer + Reporter
  render/                   the Renderer trait, Palette, Styled
  json.rs                   the --json reporter (the Answer, not the report)
  diagnosis.rs              Diagnose
crates/vvv-tui/src/
  render/                   Painter and the widgets a screen draws with
  view.rs                   Compact, the picker's view
  screen/                   one module per screen; the report overlay in
                            overlay.rs
```

`report/` is pure: no `Workspace`, no I/O, no interface. It is data crossing to
a client, which is the test `protocol/` passes.

## The report

```rust
// vvv-engine/src/report/mod.rs

/// A command's result as data, in two streams.
#[derive(Debug, Default)]
pub struct Document {
    body: Vec<Block>,
    notes: Vec<Block>,
}

/// One fact, named for what it is so a view can place it.
#[derive(Debug, Clone)]
pub enum Block {
    /// The command and its intent: its own line, then a blank.
    Title(String),
    /// A section label.
    Heading(String),
    /// Found rows — hits — that a view numbers and groups.
    Matches(Vec<Match>),
    /// Occurrences a view judges: a reference, a rename. `files` is the plan,
    /// when there is one, so a view can mark the rows an edit touches (`±`).
    Verdicts { occurrences: Vec<Occurrence>, files: Option<Vec<FileChange>> },
    /// Import sites worth a look: unresolved, then unused, then redundant.
    Imports { unresolved: Vec<ImportSite>, unused: Vec<ImportSite>, redundant: Vec<ImportSite> },
    /// What a file imports, grouped by package; `own` is its own, to mark.
    DepGroups { imports: Vec<Dep>, own: Option<String> },
    /// Who imports a file.
    Importers(Vec<Importer>),
    /// What is at a position, and how it is reached.
    Explanation(Box<Explanation>),
    /// A file's declarations as a tree.
    Outline(Vec<OutlineItem>),
    /// Where a name is declared: the sites `where` found.
    Sites(Vec<protocol::Site>),
    /// Declarations nothing refers to, and the unsure-token count of each.
    Dead(Vec<Unreferenced>),
    /// A package's exposed names, and how many import each.
    Exposed(Vec<Exposed>),
    /// The modules that import a declaration, nearest first.
    Consumers(Vec<Consumer>),
    /// The ledger, oldest first.
    History(Vec<HistoryEntry>),
    /// A batch's steps, in order.
    Batch(Vec<Intent>),
    /// What `undo` reversed: moves, then restored files.
    Undo { moves: Vec<(PathBuf, PathBuf)>, restored: Vec<PathBuf> },
    /// A bare line.
    Line(Line),
    /// The closing summary: what happened, and what to do next.
    Summary(Line),
    /// A hint or a warning.
    Note(Note),
    /// The change a plan would make: one file's edits, laid out as a diff.
    Changes(Vec<FileChange>),
    /// A move preview: the changes, and what a plan re-spelled or left by hand.
    Moved { files: Vec<FileChange>, respellings: Vec<Respelling>, notices: Vec<Notice> },
    /// A vertical gap.
    Blank,
}
```

Every variant is a fact; none carries a row a command laid out. A view turns
each block into rows through the builder for it (`Sections`, `Verdicts`,
`OutlineTree`, `DepGroups`, `ImporterRows`, `Respellings`, `NoticeRow`,
`HistoryLine`, `PlacedLine`, `ImportSiteLine`, `Caret`, `Diff`, …). `Line`, `Role`, `Mark`, `Plural`, `Files` and `Ago` stay
in `protocol::{display, vocabulary}`: they are the row vocabulary the views
share.

A composition still builds a few `Line`s by hand — a module header, an address
pair, declaration `●` rows, the `deps` counts, `impact`'s title — and pushes
them into `body`. Those are the lines every view agrees on, so they need no
block.

### A row and its source

```rust
/// A row: the line to draw, and the source it stands for.
#[derive(Debug, Clone)]
pub struct Row {
    pub line: Line,
    /// Where the row is, when an interface can act on it.
    pub source: Option<Source>,
}

/// A source site: the file and the line in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub line: u32,
}
```

`Row::new` is a row nothing can act on; `Row::at` carries a source. The picker
uses the source to put a cursor on the row and open it; the CLI ignores it.

### `body` and `notes`, not `out` and `err`

The two streams are _result_ and _note_ — semantics, not streams. The CLI maps
`body` to stdout and `notes` to stderr; the picker maps `body` to panels and
`notes` to its status line. Nothing in `report/` is named for a CLI stream.

## The view

A pure capability: a document, the room it has, and the flags it was asked for,
in; rows out. It names no command, so both interfaces hold one.

```rust
// vvv-engine/src/report/view.rs

/// The rows to draw: the result stream and the note stream.
pub struct Presentation {
    pub body: Vec<Row>,
    pub notes: Vec<Row>,
}

/// How a report is shown. One per way of seeing it.
pub trait View {
    /// Lay one block out: `width` columns (`usize::MAX` when the terminal
    /// clips), and the flags the command was asked for.
    fn rows(&self, block: &Block, options: Options, width: usize) -> Vec<Row>;

    /// Lay a whole report out, block by block. The default; no view overrides it.
    fn present(&self, report: &Document, options: Options, width: usize) -> Presentation { … }

    /// Lay one occurrence out as the picker's list row, ticked or not.
    fn occurrence(&self, occurrence: &Occurrence, ordinal: usize, ticked: bool, width: usize) -> Row;
    /// Lay one re-spelling out as the picker's list row.
    fn respelling(&self, respelling: &Respelling, width: usize) -> Row;
    /// Lay one notice out as the picker's list row.
    fn notice(&self, notice: &Notice, width: usize) -> Row;
    /// Lay one rewrite match out as the picker's list row.
    fn rewrite(&self, m: &Match, after: Option<&str>, ordinal: usize, ticked: bool, width: usize) -> Row;
}
```

Two views:

- **`Detailed`** (`vvv-engine::report`) — the terminal's: a block per line,
  notes prefixed, exactly what a command prints. The CLI styles its
  `Presentation` with `Palette` and writes it; the picker draws it into a
  ratatui `Buffer`.
- **`Compact`** (`crates/vvv-tui/src/view.rs`) — the picker's: one tight row per
  hit, no file headers, and per-fact rows for a rename's verdicts and a move's
  paths. It delegates every block it does not specialise to `Detailed`.

`View` takes `Options` because folding is a view decision: `-v` expands a
rename's verdicts and an outline's reach at draw time, not composition.

`--json` is _not_ a view: it serializes the `Answer`, a different contract, so
the CLI keeps a separate `Json` reporter.

## The composition

`Document::of(&Answer, Options)` dispatches over the variants; each is a private
method on `Document` in `report/mod.rs` (`Document::search`, `Document::rename`,
`Document::moved`, …). A composition names the facts and nothing else:

```rust
// vvv-engine/src/report/mod.rs

fn references(result: &References) -> Self {
    let mut report = Self::new();
    report.declarations(&result.declarations);
    report.block_body(Block::Verdicts {
        occurrences: result.occurrences.clone(),
        files: None,
    });
    report.block_note(Block::Summary(Self::verdict_counts(&result.occurrences)));
    report
}
```

No composition decides a column, a glyph or a colour. The rows a command prints
come from the same builder every view calls
(`Verdicts::new(..).expanded(options.verbose).planned(..)`).

`Document::error(&Failure)` is the other entry: a failure as `✗ message` and its
hints, which both reporters print.

## JSON stays the protocol

`--json` is a documented contract consumed by agents (`docs/protocol.md`). It
serializes the `Answer`, not the `Document`. A report-shaped JSON would be a
presentation document (rows, marks, columns) — a different, weaker contract. So
the CLI keeps two reporters: `Human` (compose the `Document`, lay it out, style
it) and `Json` (serialize the `Answer`).

## What the picker does

The picker shares the `View`, not the `Document`. A mode is sent the facts it
draws (`Planned::Rename`, `Planned::Move`) and calls `View::occurrence` /
`respelling` / `notice` / `rewrite` per fact, because its panels (per confidence,
per kind) are not the CLI's single document. A panel that holds one row per fact
— a move's `±` structural files, a rewrite's matches — has no `Detailed`
variant; its detail pane carries the hunk or the diff.

- `v` switches `Compact`/`Detailed`; the model holds the choice.
- Rewrite's matches panel is the hit per row; its detail pane draws the current
  match's file diff from the plan's `Block::Changes`, scrolled to the hunk
  holding the match, so a multi-line rewrite and a file with several hunks need
  no special case.
- After an apply the worker composes the `Document` and sends it as
  `Event::Applied`; `Overlay::Report` draws it and walks its source rows —
  `j`/`k` move the cursor over the rows that carry a `Source`, `e` opens one.

## Rules

- **The report is the result, the view is how it is shown.** `report/` knows
  `Answer` and the row vocabulary; it names no interface. A `View` impl names
  its interface and no command.
- **A view names only `Document`, `Block`, `Row`, `Source`, `Note`, `Line`,
  `Role`, `Mark`.** It does not import `Answer`, `Match`, `Occurrence`, or any
  command type.
- **A block is command-shaped, not content-shaped.** It holds its command's
  result type (`Exposed`, `Consumers`, `Dead`, `Imports`) or the fact the
  command found (`Matches`, `Verdicts`, `Changes`). Sharing between commands
  lives in the `lines.rs` builders (`PlacedLine`, `ImportSiteLine`, …), not in
  shared block types; `Verdicts` is shared because `references` and `rename`
  hold the same `Occurrence`s.
- **`--json` is the `Answer`.** The report is never serialized.
- **A builder is a type with a method, not a free function.**
