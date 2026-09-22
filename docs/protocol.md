# JSON protocol

Pass `--json` to any command. Output is a single JSON document on stdout, including
errors, so a client never needs stderr. Exit code is `0` for `ok`, `1` for `error`.
Or run `vvv serve` and send the same commands as JSON, one per line (below).

The types are defined in `vvv_engine::protocol` (`crates/vvv-engine/src/protocol/`); a
Rust client depends on `vvv-engine` alone and gets them alone. This
page is the human-readable contract. Field order is not significant. Absent optional
fields are omitted, not `null`.

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

## `vvv outline`, `references`, `where`, `deps`, `explain`

Read-only answers built from the same declarations and imports a rename uses. Each is
a plain structure of the shared types; `Symbol` fields are flattened into an outline
item.

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
real tree, the steps before it are rolled back and nothing is recorded. History records
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

Between the two runs the engine recomputes everything; ids are the only state
carried. If a file changed in between, an id no longer resolves and the command
errors with `no match with id(s) …` rather than acting on a different span. Row
numbers work the same way for a human at a terminal; a program should prefer ids,
which do not depend on the result order.
Likewise a plan refuses to apply to a file whose contents differ from when it was
previewed (`… changed since the plan was made`).

## Errors

`code` is one of these and does not change between releases; `message` says what
happened in words and `hint` (when present) what to try, and neither is for parsing:

| code               | meaning                                                                           |
| ------------------ | --------------------------------------------------------------------------------- |
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
| `stale`            | a file changed since the plan (or the apply to undo) was made                     |
| `no_history`       | nothing to undo, or a history file that cannot be read                            |
| `io`               | reading or writing the tree failed                                                |
