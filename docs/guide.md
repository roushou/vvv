# User guide

This guide covers how each command decides what to touch, what the marks in the output
mean, and where the edges are. For the full list of flags, `vvv <command> --help` is
always current.

## Reading the output

Every command draws from the same small set of marks, so once you know them you can
read any screen at a glance:

| mark        | meaning                                           |
| ----------- | ------------------------------------------------- |
| `●`         | a declaration                                     |
| `→`         | an import (or, in a preview, what a path becomes) |
| `←`         | imported by                                       |
| `↗`         | a re-export                                       |
| `◎`         | placed by an oracle (a build), not by syntax      |
| `✓` `?` `✗` | will be changed / can't tell / belongs to another |
| `!`         | needs your hand                                   |
| `±`         | a structural edit — a `mod` line, a visibility    |
| `◆`         | a module address (`crate::a::b`)                  |
| `∅`         | nothing                                           |
| `+N`        | the match continues for N more lines              |

Declarations always come first. Rows are numbered in the order printed; those numbers
are what `--select` takes.

## Options that work everywhere

| flag                          | what it does                                                                                                                 |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `-C <dir>`                    | Run against a different project root. The default is the current directory.                                                  |
| `--json`                      | Print a machine-readable structure instead of the human view. Errors come back the same way. See [protocol.md](protocol.md). |
| `--color auto\|always\|never` | `auto` (the default) colours output only when it's going to a terminal. `NO_COLOR` is respected.                             |
| `-v`                          | Expand what the human view collapses — every `✓` row of a rename instead of counts per file; each item's reach in `outline`. |
| `--diff`                      | Print a preview's full patch, not only its structural (`±`) edits.                                                           |

Results go to standard output and everything else — summaries, hints, warnings — goes
to standard error. That means `vvv search Config | grep impl` does what you'd hope.

## Searching

You can describe what you're looking for in three ways, and they stack.

### By name

A plain word finds every identifier spelling it, wherever it appears. The declaration
comes first, with its address; then every other use, grouped by file, imports marked
`→`:

```console
$ vvv search Config
● 1  1 file
src/util/parse.rs
  1 ●    5:12   struct Config  pub struct Config;  ◆ app::util::parse::Config

○ 2  1 file
src/net/client.rs
  2 →    1:32   use super::super::util::parse::Config;
  3      3:36   impl Client { pub fn cfg(&self) -> Config { Config } }
```

The last line, on standard error, is the count: `● 1  ○ 2  2 files`. When a search
finds only declarations (`--symbol`) or only uses (a pattern), the section headers are
left out and the list is the whole answer.

This is a whole-token match: `Config` won't find `Configuration`, and it won't find the
word inside a string or a comment.

### By what it declares

`--symbol` picks declarations by kind and `--name` by name. The kinds are
`function`, `method`, `struct`, `class`, `enum`, `variant`, `trait`, `interface`,
`type-alias`, `const`, `static`, `variable`, `field`, `module`, `macro` and `impl`
(Rust `impl` blocks, named after their type; they only appear when asked for).

```console
vvv search --symbol trait            # every trait
vvv search --symbol method --name new   # every method called new
```

### By shape

A pattern in [ast-grep syntax](https://ast-grep.github.io/guide/pattern-syntax.html)
matches code by structure. `$X` stands for any single node and `$$$X` for any sequence
of them. The pattern has to be a complete construct on its own — `fn $N($$$) { $$$ }`
works, `fn $N` doesn't, because the parser needs something it can parse.

```console
vvv search 'impl $TRAIT for $TYPE { $$$ }'
vvv search 'unwrap_or($X.clone())'
```

`--kind` narrows to a tree-sitter node kind (`function_item`, `impl_item`, and so on)
and `--lang rust|typescript|tsx` limits the search to one language. All of these can be
combined; a match has to satisfy every part.

### Asking about structure

Search says _where_; these say _what_. They read the same declarations and imports a
rename uses, so what they print is what a rename would act on.

```console
vvv outline src/plan/mod.rs             # what the file declares, with visibility and address
vvv references Plan                     # every token spelling Plan, judged like a rename would
vvv where Plan --from src/lib.rs        # where Plan is declared and the `use` to write in lib.rs
vvv deps src/plan/mod.rs                # what the file imports, and who imports it
vvv explain src/plan/mod.rs:49:12       # what is at that position: declaration, module, reach
```

Each opens with the module once — `◆ vvv_core::plan   src/plan/mod.rs` — and never
repeats it per row.

`outline` is a tree: nesting by containment, functions and methods as `name()`, fields
and variants bare, the modifier as written after the name (`·` when there is none,
which is what private looks like):

```console
$ vvv outline src/plan/mod.rs
◆ vvv_core::plan   src/plan/mod.rs

  24  enum ApplyError   pub
  26    Vfs             ·
  41  struct Plan       pub
  47  impl Plan
  49    new()           pub(crate)
  71  check()           ·
```

`-v` adds who may name each item (`everyone`, `package vvv_core`, `within
vvv_core::plan`). The count on standard error is per modifier: `● 6   pub 2  pub(crate) 1  · 2`.

`deps` is two lists, `→` what the file imports grouped by the package it leads into
(the file's own first, `?` for a crate no manifest names; `std`, `core` and `alloc`
are always known), one row per statement with the file it leads to — the file
_declaring_ the thing, followed through re-exports, so `use vvv_core::Span` leads to
`text/span.rs`, not `lib.rs`; and `←` who imports it, one row per statement with the
names taken, counting a file that takes one of this file's declarations under an
address a `pub use` offers it at:

```console
→ 3
  vvv_core
    10  crate::resolve::{Address, PackageId}   → crates/vvv-core/src/resolve/mod.rs
    11  crate::symbol::SymbolKind              → crates/vvv-core/src/symbol/mod.rs
  serde
     8  serde::{Deserialize, Serialize}

← 5   5 files
  crates/vvv-core/src/answer/mod.rs:11    Reach
  crates/vvv-core/src/lib.rs:46           Reach ReachKind Semantics VisibilityRule
```

`explain` takes an editor-style `path:line[:column]`, 1-based, and shows the enclosing
declaration's line with a caret under its name, then the same `●` line `search` prints,
who may name it, `↗` every other address a re-export offers it at, and `←` the files
importing it at any of them:

```console
crates/vvv-core/src/semantics.rs:40:12
    39 │ pub fn reach_kind(&self, modifier: Option<&str>) -> ReachKind {
       │        ^^^^^^^^^^

● method reach_kind   pub   ◆ vvv_core::semantics
  reaches  everyone
← 5
  crates/vvv-core/src/answer/mod.rs
  …
```

On an import statement it says where the thing really comes from — what the path
spells, then `↗` the declaration a re-export chain leads to and its file:

```console
crates/vvv-engine/src/change.rs:12:20
→ vvv_core::ChangeSet   ◆ vvv_core::ChangeSet
  ↗ vvv_core::edit::change_set::ChangeSet   crates/vvv-core/src/edit/change_set.rs
◆ vvv_engine::change
```

`where` prints one `●` line per declaration and, with `--from`, the import to write
as a `→` line under it. `references` prints a rename's preview without the plan — the
same `✓ ? ✗` sections and tags — and takes the same `--in`, `--symbol` and `--lang` as
`rename`. Imports that go through a re-export (`use vvv_core::Span` for a `Span`
declared in `vvv_core::text::span` and re-exported up) are followed to the
declaration and count as `✓`, tagged `↗ re-export` in `-v`. A re-export under another
name (`pub use a::X as Y`) is followed too: `Y` reaches `X`'s declaration, and a rename
of `X` leaves `Y` alone.

If a pattern does not parse in one of the project's languages, that language is skipped
with a warning and the others' matches are complete; pass `--lang` to ask one language.

### Asking about the whole tree

These walk every file's declarations and resolved imports at once rather than one
file's. They answer with plain data and count what vvv could not judge instead of
hiding it.

```console
vvv surface vvv_core            # what a package offers: pub items, where re-exports offer them, who takes them
vvv impact Span                 # who would feel a change: importers, then their importers, outward
vvv dead --lang rust            # declarations nothing in the workspace refers to
vvv imports src/plan/mod.rs     # imports worth a look: unresolved, unused, redundant
```

`surface` prints one `●` line per public declaration (or one a `pub use` offers on,
whatever its own modifier) with `← n` files importing it at any of its addresses, and
an `↗` line per address a re-export chain offers it at:

```console
$ vvv surface vvv_core
surface vvv_core

● struct Span   ◆ vvv_core::text::span::Span   crates/vvv-core/src/text/span.rs:9:12   ← 26
  ↗ vvv_core::text::Span
  ↗ vvv_core::Span
```

`impact` is rings: depth 1 is every module importing the declaration (or an address
that re-exports it), depth 2 every module importing one of those, and so on to depth
8. A module is listed once, at the first depth it is reached, with `via` the module
that carried it there:

```console
$ vvv impact Plan
impact Plan   ◆ vvv_core::plan::Plan

← 2   depth 1
  ◆ vvv_core::workspace   src/workspace/mod.rs
  ◆ vvv_core::lib   src/lib.rs

← 1   depth 2
  ◆ vvv_core::engine   src/engine.rs   via vvv_core::workspace
```

`dead` asks, for every addressable declaration, the question `references` asks: is
there a `✓` token other than its own name? Items with none are listed, with `? n` when
some tokens could not be judged — those may be uses after all. It is honest rather
than clever: `#[test]` functions, `main` and `#[cfg(test)] mod tests` are listed too,
since nothing in the tree names them; and a trait imported for its methods looks
unused to it. Read the list as questions, not verdicts.

`imports` is three sections per file: `∅ unresolved` (the layout could not place the
path — a package no manifest names, or an inline module's `super`), `! unused` (the
name the statement binds, its `as` alias if any, is never spelled again in the file)
and `! redundant` (another statement in the file already brings the same address in;
a glob of `X` and `X` itself are different things). Files the layout cannot place at
all — a crate's integration tests — are counted as `not placed` rather than judged.
Judgement is per file, not per scope, so two test modules importing the same name
count as redundant.

## Choosing what a command acts on

Every row in a search is numbered, and `--select` takes those numbers — single rows,
ranges, or both:

```console
vvv search 'foo($$$A)'                                  # look at the rows
vvv rewrite 'foo($$$A)' 'bar($$$A)' --select 2,5-7 --apply
```

The numbers are positions in the same search run again, so they mean the same thing as
long as the code hasn't changed in between; if it has, the plan's fingerprint refuses
to apply rather than acting on whatever is there now. `--json` output carries a
content-derived `id` per match instead (`827aff1a882c`, computed from the file, the
position and the text), and `--select` accepts those too — for scripts and agents that
keep results across runs. Without `--select`, a command acts on all of its matches.

## Previewing, applying and undoing

None of `rewrite`, `rename` or `move` writes anything on its own. Each one works out a
plan, prints it, and stops; the last line on standard error is the plan's size and the
flag to go on with (`± 2   2 files` / `hint: --apply to write`). When you add
`--apply`, the plan is written all at once — and if any of the files changed between
the preview and the apply, vvv stops and tells you instead of writing over the changes.
What you get back is a receipt naming the history entry: `✓ #3   ± 2   2 files`.

Each apply is saved to `.vvv/history.json` (worth adding `.vvv/` to `.gitignore`).

```console
$ vvv history                    # oldest first; ↩ marks what undo reverses
#1    2 h ago  move src/a.rs → src/b.rs  3 files
#2  just now  rename Config → Settings  5 files  ↩
$ vvv undo
↩ #2  rename Config → Settings

  src/main.rs
  src/util.rs
```

Undo restores files to exactly what they were, moves included (a moved file shows as
`src/b.rs → src/a.rs`). If you've edited one of those files since the apply, undo
refuses and leaves the history entry in place, so you can sort it out and try again.

Errors start with `✗` and are followed by `hint:` lines when there is an obvious next
thing to try; an empty answer is `∅`.

## Rewrite

`vvv rewrite <pattern> <template>` replaces every match of the pattern with the
template. Anything the pattern captured can be used in the template:

```console
vvv rewrite 'unwrap_or($X.clone())' 'unwrap_or_else(|| $X.clone())'
```

A captured sequence like `$$$ARGS` comes through with its original spacing and commas.

## Rename

```console
vvv rename <name> <new-name> [--symbol <kind>] [--in <file>]
```

Rename finds the declaration, then every identifier in the same language that spells
its name, and works out for each one whether it really refers to that declaration. The
preview is three sections:

```console
$ vvv rename Reach Scope
rename Reach → Scope

● enum Reach   ◆ vvv_core::semantics::Reach   crates/vvv-core/src/semantics.rs:105:1

✓ 29  4 files
  15  crates/vvv/src/output/fixtures.rs
   3  crates/vvv-core/src/answer/mod.rs
   1  crates/vvv-core/src/lib.rs
  10  crates/vvv-core/src/semantics.rs

? 4  1 file   ∅ unresolved   in the plan
crates/vvv-engine/tests/answers.rs
  30     11:58   use vvv_core::{Address, Confidence, MemoryVfs, Position, Reach, …};
…

✗ 0
```

| section | what vvv found                                                                                   | renamed by default?                                 |
| ------- | ------------------------------------------------------------------------------------------------ | --------------------------------------------------- |
| `✓`     | It's the declaration itself, or the file imports it, or it's reached by a path that leads to it. | yes                                                 |
| `?`     | Nothing connects it to the declaration — see the tag.                                            | only when nothing else in the project has this name |
| `✗`     | It refers to a _different_ declaration that happens to have the same name.                       | never                                               |

`✓` rows are the ones you don't need to read, so they collapse to a count per file;
`-v` lists them. `?` and `✗` rows are listed in full, under a tag saying why:

| tag            | meaning                                                                                                                          |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `∅ unresolved` | A bare name with no import, or a path vvv can't place (an unknown crate, a type's associated item, a token inside a macro body). |
| `∅ by name`    | A method or field — reached through a type, so there is nothing to judge by; the match is by name.                               |
| `● other decl` | Resolves to another declaration of the same name in this workspace.                                                              |
| `→ external`   | Resolves into a crate outside the workspace, which cannot be yours.                                                              |

The `?` header says whether those rows are `in the plan` or `left out`; the hint on
standard error gives the `--select` range that flips it. Rows are numbered in the
order shown — `✓`, then `?`, then `✗`, each grouped by tag — so a section is always
one range.

If the name is declared in more than one place, vvv doesn't guess. It lists the
declarations (one `●` line each) and asks you to choose one with `--in <file>`. You
can always override the defaults with `--select`.

All of this works by following imports and module paths, not by knowing types. That's
enough for functions, structs, enums, traits, type aliases, constants and modules. It
isn't enough for methods and fields, which you reach through a type: their occurrences
are all `? ∅ by name`, and `--select` is how you narrow them down.

In a Cargo workspace, paths into another crate resolve — `use fff::Config` where
`fff` is a member, under whatever name the manifest gives it — so a rename crosses
crates.

## Move

```console
vvv move <from> <to>
```

Moves a file or a directory, then rewrites every import that pointed at anything inside
it, along with the moved files' own imports. Renaming as you go is fine: `vvv move
src/util src/core/tools` both relocates and renames the module.

The preview separates what is mechanical from what you should read:

```console
$ vvv move crates/vvv-core/src/semantics.rs crates/vvv-core/src/resolve/semantics.rs
move crates/vvv-core/src/semantics.rs → crates/vvv-core/src/resolve/semantics.rs

◆ vvv_core::semantics → vvv_core::resolve::semantics

→ 5   5 files
± 3   3 files

→ crates/vvv-core/src/answer/mod.rs:11    crate::semantics::Reach → crate::resolve::semantics::Reach
→ crates/vvv-core/src/lang/mod.rs:24      crate::semantics::Semantics → crate::resolve::semantics::Semantics
→ crates/vvv-core/src/lib.rs:46           semantics → resolve::semantics
…

± crates/vvv-core/src/lib.rs
@@ -18,7 +18,6 @@
 pub mod search;
-pub mod semantics;
 pub mod symbol;

± crates/vvv-core/src/resolve/mod.rs
@@ -10,6 +10,7 @@
 mod project;
+pub mod semantics;

± crates/vvv-core/src/semantics.rs → crates/vvv-core/src/resolve/semantics.rs
```

`◆` is the module address before and after. Every path re-spelled in place is one `→`
row (the changed segment is bold in a terminal); anything else — a `mod` line moving, a
visibility widened, the file itself — is a `±` hunk, because that is what you want to
read. An import vvv understood but could not rewrite is a `!` row with the replacement
it would have written. `--diff` prints every file's full patch instead.

**Rust.** A module is its `foo.rs` file and its `foo/` directory together. Name either
one and both move. The `mod foo;` line moves from the old parent file to the new one,
which has to exist already (`src/core.rs` or `src/core/mod.rs`); if it doesn't, vvv
tells you which file to create. After the paths are rewritten, vvv checks that every
reference the move touched can still see what it names — every `mod` on the way and
the item itself — and widens only what some consumer can no longer reach, only as far
as it must: `pub(super)` when every consumer sits under the parent, `pub(crate)`
otherwise. It never writes `pub`: a consumer in another crate becomes a notice, because
publishing an item is your decision. Nothing is ever narrowed, and a `mod` that nobody
outside needs moves as written. Paths starting with
`crate::`, `self::`, `super::` or a child module's name are rewritten, and vvv keeps
the relative style when it still means the same thing. Grouped imports like
`use a::{b, c::d}` are updated in place when the group still covers the target, and
split into their own `use` line when it doesn't.

**One declaration, not the file.** `vvv move --symbol Config src/util.rs src/config.rs`
moves the declaration called `Config` — with its doc comments and attributes and, in
Rust, the `impl` blocks for it — to the end of an existing file of the same language.
Every consumer follows: `use` paths and qualified paths are rewritten, a `pub use`
re-export points at the new place, the old file imports it back if it still uses it,
and an import of it in the new file is removed. The imports the moved text relied on
come along (re-rendered from the new file, skipping what it already imports), a sibling
the text names is imported and widened if the new file could not see it, and the
declaration itself is widened for the consumers that could no longer reach it — the
same rules as a file move. What stays behind is left alone: imports the old file no
longer needs become compiler warnings, not edits. Not followed: glob imports of the
old module (`use util::*` then a bare `Config`), which become a compile error to fix by
hand.

Not handled: modules declared with `#[path]` or inline `mod x { }`, files under
`src/bin` and `tests/`, moving between crates, and `lib.rs`, `main.rs` or `mod.rs`
themselves.

**TypeScript.** Relative imports (`./x`, `../y`) are rewritten, including ones that
resolve through extensions or an `index` file, and an import written as `./x.js` for a
`.ts` file keeps that spelling. `tsconfig` path aliases and `package.json` `exports`
are not handled.

## Several changes as one

`vvv batch` takes a JSON array of intents — the `intent` objects `--json` prints — and
plans them in order, each against the tree as the previous one leaves it:

```console
$ cat steps.json
[ { "command": "move",   "from": "src/util/parse.rs", "to": "src/net/parse.rs" },
  { "command": "rename", "name": "Config", "to": "NetConfig", "declared_in": "src/net/parse.rs" } ]
$ vvv batch steps.json            # one preview of the end state
$ vvv batch steps.json --apply    # one transaction, one history entry, one undo
```

Nothing real is written while planning; the steps run against a staging copy. On
apply, a step that no longer holds (a file changed under it) rolls the earlier steps
back. Agents get the same through `--json`; `-` reads the array from stdin.

## The picker

Run `vvv` with no command, or `vvv ui`, to get an interactive session. It is built
from **modes** — search, rename, move, rewrite, history — each with its own layout and
keys, and every layout is made of **panels**: a bordered list of one kind of thing,
whose title is its legend and its count. The panel with the bright border is the one
your keys go to; `tab`/`shift-tab` cycle panels, `1`–`5` jump to one, and the status
bar at the bottom names the mode, then the keys that matter in that panel — with `⏎`
always spelled out. In an **input** panel (the query, a new name, a rewrite
template) a plain key types; actions live on modified keys, so `ctrl+n`/`ctrl+p`
walk the results without leaving the query. A `?` in a pattern (a Rust `?`, a
TypeScript `x?: T`) is just text. `esc` goes back one step, ultimately to search;
`e` opens `$EDITOR` at the row under the cursor; `?` on a list or detail shows the
marks and every key that works where you are, by where it comes from (`j`/`k` scroll
it, any other key closes it) — the query itself has none, so `tab` to a list for it.
The status bar and that list are two views of one table, so they never disagree. The picker keeps what it has read between keystrokes and re-reads only files that changed;
it looks at the tree again at most once a second while you type, and always right
after the editor returns or it writes something itself.

**Search** is the hub: the query in the title, the rows below (`●` declarations first,
`→` imports dimmed, the file and line shortened to `dir/file.rs:line`), and a context
panel on the right that says what the row is — `● struct Engine  pub  ◆ vvv_engine`
and its uses for a declaration, the declaration a use resolves to — then the source
around it. Filter words (`symbol:trait`, `name:Foo`, `kind:impl_item`, `lang:rust`)
go anywhere in the query; `s` and `L` pick them from a list. The results list is
where you act: `↓` or `⏎` from the query (from the context, `esc` or `←`), then the
letters — `r` renames what is under the cursor, `m` moves its file, `M` moves the
declaration, `w` rewrites the search's matches, `h` opens history, `u` undoes the
newest apply. `v`
switches the rows between the compact list and the full report the CLI prints —
search results, a rename's verdict rows, a move's paths and notices.

**Rename** shows the new name in the title — type to change it and every `after` text
follows — and a panel per verdict: `? unverified` (largest, where judgment is needed),
`✓ safe`, `✗ another declaration's`.
Each row is a site with a checkbox (`▪` ticked, `▫` not), the reason glyph, the place
and the line; `space` flips one, `a` flips every row of the focused panel. The
checkboxes start where the CLI's default would act. The detail panel shows the line
before and after, the reason unfolded, and the source around it. `⏎` writes exactly the
ticked rows.

**Move** takes the destination in the title and plans it as you type: the panels below
(`→ paths rewritten`, `± structure` — the `mod` line, the file itself — and
`! by hand`) fill when the destination is valid, and the title says
why when it is not. The detail panel shows the respelling and the line, or the hunk;
`d` switches between source and hunks. `⏎` applies.

**Rewrite** keeps the search as its pattern and takes the template in the title; each
match is a row, and the right panel shows the file's diff — the same hunks the CLI
prints — scrolled to the hunk holding the match, updating as the template expands.
`⏎` writes the ticked rows.

**History** is the ledger, oldest first, `↩` on the entry `u` reverses; the right panel
lists the files that entry touched.

After an apply the picker returns to search, re-runs the query so the rows show the
new state, and reports the receipt (`✓ #3  rename Reach → Scope`) in the status bar.
The apply's report is on screen as a box: `j`/`k` walk its source rows, `e` opens
the row under the cursor in `$EDITOR`, and any other key closes it.

## Driving vvv from a program

`--json` on any command prints one JSON document (see [protocol.md](protocol.md)).
For many commands in a row — an agent, an editor — `vvv serve` keeps one session:
send `{"command": "rename", "name": "Config", "to": "Settings", "apply": true}` on
a line of stdin, read one line of stdout back, and the tree is not re-read between
requests where it did not change. Every command has the same fields as its flags;
errors come back with a `code` to branch on and a `hint` to show.

## Building with fewer languages

Language support is compiled in. The default build includes everything. If you only
want Rust:

```console
cargo install --path crates/vvv --no-default-features --features rust,tui
```
