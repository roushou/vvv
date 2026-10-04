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
comes first; then every other use, grouped by file, imports marked `→`:

```console
$ vvv search Config
● 1  1 file
src/util/parse.rs
  1 ●    5:12   pub struct Config;

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
vvv outline src/plan/mod.rs             # what the file declares, with visibility
vvv references Plan                     # every token spelling Plan, judged like a rename would
vvv where Plan --from src/lib.rs        # where Plan is declared and the `use` to write in lib.rs
vvv deps src/plan/mod.rs                # what the file imports, and who imports it
vvv navigate src/plan/mod.rs:49:12      # follow this identifier to its definition
vvv explain src/plan/mod.rs:49:12       # what is at that position: declaration, module, reach
```

Each opens with the file path once — `src/plan/mod.rs` — and never repeats it per
row.

`outline` is a tree: nesting by containment, functions and methods as `name()`, fields
and variants bare, the modifier as written after the name (`·` when there is none,
which is what private looks like):

```console
$ vvv outline src/plan/mod.rs
src/plan/mod.rs

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

`navigate path:line[:column]` follows the identifier at that position and shows its
captured declaration source. Coordinates are 1-based. For several possible targets,
use `--select 2` or `--select <match-id>` to choose a candidate. `--json` includes
versioned target anchors and the definition source; `vvv serve` accepts the same
capability as a `navigate` request.

Use `vvv navigate path:line:column --compact --json` for a small resolution result:
name, match ID, versioned target, display container, declaration position, and
evidence. This uses the engine's `resolve` request and keeps all ambiguous
candidates. Request source separately with `context`. MCP's `vvv_navigate` uses
this compact contract; full navigation previews remain available to the CLI and TUI.

Rust navigation follows module-level imports, aliases, re-exports, and type uses
in fields, function parameters, and return types. Inline modules in files with a
known module address have their own declarations and imports, including nested
`self`, `super`, and `crate` paths, aliases, globs, and visible re-exports. Private
items stay within their visibility boundary; competing declarations remain
selectable candidates. It also follows simple function
parameters, generic type parameters, `let` locals, tuple/slice bindings, closure
parameters and captures, and supported local type/function items. Explicit type
annotations inside supported function bodies can follow module imports. Inner bindings shadow outer ones; a `let` initializer still sees the previous
binding. TypeScript follows exact named/default imports, namespace-qualified type
uses, aliases, named re-exports, local export lists, generic type parameters, and
function and method parameters, including nested object/array destructuring, renamed
fields, shorthand bindings, defaults, array holes, and rest bindings. Importing one
name does not expose other names in that module.
Declaration tokens preview themselves; enum variants preview their enclosing enum.

TypeScript/TSX parameter bindings are initialized in their written parameter and
pattern order. Defaults and computed property keys can navigate to already initialized
bindings and supported imports. References to the binding being initialized or a
later parameter/field return `unsupported_context`; they do not fall through to an
outer declaration or import. A default for an entire object/array parameter runs
before that pattern's bindings are initialized. Static property labels are not
bindings. Unsupported or unfinished patterns block confirmation for the complete
callable header without publishing a supported prefix. Pattern traversal is
bounded to 1,024 nodes and 128 levels per header. Duplicate bindings remain explicit
candidates. Direct `let`/`const` declarations in function, method, and nested blocks
support the same patterns. Their names belong to the entire block; references before
initialization return `unsupported_context` instead of falling through to outer names.
Initializers run before their patterns; earlier declarators and pattern bindings are
available to later defaults and computed keys. An initialized binding in a strictly
inner block still wins. `let name;` initializes at its declaration. Unsupported local
patterns block confirmation for the complete block without publishing a local prefix.
The traversal budget applies to each block as well as each header.

TypeScript/TSX declaration owners include file roots, callable bodies, lexical blocks,
switch bodies, class static blocks, and supported namespace bodies. File-level
`let`/`const` bindings use the same initialization policy as block locals; imports
remain available throughout their file. Files retain separate environments: script
globals are not merged across files and project configuration does not imply strict
mode. Ordinary functions and `var` declarations are visible throughout their file or
variable owner. Parameter defaults remain outside a callable's body environment.

An explicit import/export marks a module. Modules, class bodies, and inherited
`use strict` directives establish strict scope evidence. Direct functions in strict
nested/catch/switch blocks belong to that block, including forward uses; block
functions do not escape into the enclosing function. Ordinary block functions
without strict/module evidence and bare conditional function declarations retain
conservative enclosing-owner coverage rather than assume legacy runtime semantics.
Generator and async block functions retain block ownership. Each direct function
group has a 1,024-declaration bound. Overload and ambient function signatures retain
separate declaration candidates and their own header parameter/type scopes; no
signature or implementation is silently preferred.

Local classes and enums retain distinct type and value bindings; interfaces and type
aliases introduce type bindings. Class value bindings remain uninitialized before
the declaration completes, while the class body owns its initialized self-name.
Enum bodies own their member names and self-name; self/forward member initializers
retain uninitialized ownership rather than fall through to outer values. Unsupported
member names reject the entire member group.
Class-expression names stay inside that expression; generic type parameters can
shadow a class name in the type namespace. Switch cases share one lexical scope,
excluding the scrutinee. A lexical binding is not assumed initialized in another
case, even when that case appears later in source order.

Class static blocks and simple, non-ambient namespace bodies have separate `var`
owners, pruning nested callables/classes/namespaces. Locals do not escape into
sibling static blocks, methods, or enclosing files. Merged, ambient, string-named,
and qualified namespace bodies remain conservative; namespace-member inference and
namespace import/export resolution are outside this contract. Conflicting imports,
lexical/function declarations, nested `var`/lexical declarations, and destructured
catch/`var` bindings block confirmation in the affected owner. Simple catch bindings
retain their distinct catch scope when an enclosing `var` repeats the name. A
`with` statement remains unsupported. All these facts are navigation-only and do
not authorize additional mutations or infer runtime values, branch execution, or
invocation time.

TypeScript/TSX arrows (including single parameters and expression bodies), function
expressions, and generator declarations/expressions support the same parameters and
patterns. Initialized inner bindings take precedence over an uninitialized outer
parameter, including inside a parameter default. Named function-expression bindings
belong only to that expression; its parameters can shadow the name. Supported outer
bindings remain visible as captures, without inferring invocation time or runtime
callback targets. Unsupported `var` owners block only their callable body.

`var` declarations in supported TypeScript/TSX variable owners own the complete environment,
including nested blocks, classic `for`, `for…in`, `for…of`, and `for await…of` headers.
Names resolve before their declaration and in initializer/default/computed-key uses;
this identifies source declarations without establishing runtime values or execution
order. Callable parameter defaults remain outside the body scope. Nested callables
have separate owners; class, type, and namespace boundaries are not traversed.
Object/array patterns use the same checked labels, defaults, holes, and rest structure,
without lexical temporal dead zones. Repeated `var` declarations remain selectable
candidates. Compatible parameter, `var`, and direct function redeclarations retain every written
source site as a selectable candidate. Parameters without default or computed-key
expressions share the body environment, including rest/destructured parameters.
Parameter expressions create a separate environment: body declarations take precedence
only inside the body, while header defaults retain parameter ownership. This is binding
identity evidence, not inference of runtime values or the last executed assignment.
Conflicts with lexical declarations remain unsupported. Malformed/unsupported patterns and legacy initialized iteration headers
publish no `var` binding prefix and block the body. The ownership walk visits at most
1,024 named nodes with a 128-level bound; pattern traversal has the same separate
bounds. Collection runs once per body containing a `var`, and custom/competing grammar
rules retain table interpretation. File roots and static/namespace bodies use the same bounded collection.

Catch bindings support identifiers, nested destructuring, ordered defaults, and
rest bindings. Their names belong only to the catch clause; optional-binding catch
clauses preserve outer bindings. Unsupported headers publish no binding prefix and
block their callable or catch owner. Traversal uses the same 1,024-node and 128-level
bounds. These facts are navigation-only. Anonymous caller identities remain unsupported.

TypeScript/TSX `for (let/const …; …; …)`, `for…in`, `for…of`, and `for await…of`
headers use the same ordered pattern interpretation. Header bindings belong to the
loop, including its initializer or iterable, condition, increment, and body; they
do not escape afterward. A same-named reference in the iterable is in the binding's
temporal dead zone. Earlier initialized bindings resolve in later declarators and
pattern defaults. Unsupported headers publish no header-binding prefix and block
confirmation within the loop. Supported `var` headers belong to the variable owner rather than the loop. Assignment
iteration headers support checked object/array patterns, defaults, computed keys,
rest, and member/subscript targets without introducing declarations. Static labels
remain excluded; references retain existing scope and initialization checks. Optional
member targets and malformed patterns remain unsupported. Per-header traversal has the same 1,024-node and 128-level bounds.

Rust tuple-struct patterns such as `Some(value)` extract their supported inner
bindings without treating the constructor as a local binding. In `let … else`,
those bindings become visible after the complete declaration; its initializer and
`else` body retain the outer bindings. Nested unsupported patterns retain the
conservative scope barrier. Constructor resolution still requires independent
navigation evidence; pattern extraction does not infer types or expand macros.

Rust struct patterns extract shorthand fields, renamed fields, nested supported
patterns, `ref`/`mut` bindings, and `..`. Constructors and explicit field labels are
not local bindings; field labels do not navigate as same-named outer variables.
The rules apply to function/closure parameters, let declarations, conditional and
loop bindings, and match arms. Unsupported or malformed nested fields reject the
whole pattern without publishing a supported prefix. Reference-binding evidence
belongs to each name, without proving its type or expanding macros. These facts
remain navigation-only and do not add field rename or struct-pattern mutation.
Bindings that share one pattern owner have distinct declaration IDs derived from
their exact name tokens, so ordinal and ID selection identify the chosen binding.
Their symbol spans and display containers still retain the complete owner.

Rust follows ordinary `if`/`else` branches and supported `if let`/`&&` let chains.
Condition bindings are visible to later operands and the successful branch, but
not their own initializer, an `else`/`else if` alternative, or following statements.
Inner branch locals and imports retain their precedence; closures can capture
condition bindings and nested function items cannot. Unsupported condition patterns
keep that conditional conservative while unrelated surrounding code remains
eligible. Known module and imported constants in immutable patterns resolve to the
constant rather than a new variable. This does not add conditional-pattern mutation
support.

Rust patterns also support alternatives (`Some(value) | Other(value)`), captures
(`whole @ Some(inner)`), literals, ranges, and tuple/slice rest (`..`, including
`tail @ ..` in slices). Alternatives require identical binding names and explicit `ref`/`mut` modes
after names are classified as bindings, constants, or unit constructors. Each
written binding site remains selectable.
Malformed patterns, incompatible alternatives, misplaced/repeated rest, and pattern
macros publish no partial bindings. Literals and ranges introduce no bindings.
Named range endpoints resolve only to constants; a same-named local never
provides a fallback. Constructor heads resolve through scoped imports, aliases,
globs, and re-exports, with unit/tuple/record shape evidence. Enum variants inherit
their owning enum’s visibility. Competing eligible declarations remain selectable.
Unavailable imports, unknown expansions, missing constructor evidence, and
incompatible alternatives remain conservative. Type compatibility and implicit match ergonomics are not
inferred. Alternative nesting is bounded at 128 levels; engine classification and
alternative validation share the navigation work budget.

Rust match arms extract supported identifier, tuple, slice, reference, and
tuple-struct and struct patterns. Arm bindings reach their guard and body, including capturing
closures, but do not escape into sibling arms or nested function items. Unsupported
patterns block their own arm without blocking the scrutinee or supported peers.
`for` bindings begin after the iterator expression; `while let` and ordered `while`
let chains follow the same operand visibility rules as `if`. Ordinary `while` and
`loop` bodies retain supported outer bindings. Unsupported loop patterns block that
loop. Typed and untyped closure parameters have separate bindings scoped to their
closure; nested function items cannot capture them. Macro uncertainty still applies
to these scopes, and macro arguments are not interpreted. Receiver-head navigation and
unsupported nested patterns remain outside this coverage.

Unmodeled patterns, receiver-dependent
methods, inferred targets, block-local modules, and unmodeled lexical scopes remain
unsupported. Named function/block-local imports and aliases are visible throughout
their block, including nested functions and closures. Inner imports shadow outer
bindings; same-block variables take precedence after their initializer, and
competing imported/local items remain selectable candidates. Imported constants
are recognized in immutable identifier patterns. Local globs and cases requiring
constructor namespace evidence remain conservative. Local import targets require
a file placed by the language layout. Macro arguments are not resolved.
A statement-position macro may introduce items throughout its block and locals
after its invocation, so affected lookups remain unsupported, even before the
macro when generated items could compete. Navigation can still follow proven
inner items, explicitly mutable bindings in inner scopes, and `let mut` locals in the same block before the macro or introduced
after it. Immutable identifier patterns remain conservative because they can match
an unknown generated constant instead of declaring a variable. Parameters and
outer-block bindings may also be shadowed by generated block items before the
invocation. Rooted `crate::` paths outside macro arguments remain eligible. Macros
in recognized required-expression positions
(such as call arguments and `let` initializers) do not block surrounding navigation.
No macro name is assumed safe.

Inline module lookup also requires a file placed by the language layout. `#[path]`
module mappings are not modeled. TypeScript wildcard exports, package/path aliases, arbitrary namespace
member expressions, and runtime assignment/alias targets are not resolved by this syntax path. A TypeScript default export is not treated as a named export.
Missing identifiers, unresolved names, external source, cycles, and ambiguous
definitions have distinct outcomes. A source changed since a search produces a
stale error; repeat the search to obtain current locations.

The CLI and TUI use syntax resolution. Library hosts can supply a versioned
`NavigationProvider` through `NavigationQuery::execute_with` for unresolved or
unsupported occurrences. This API does not launch a language server; hosts provide
that adapter and its position-encoding conversion. See the
[provider contract](protocol.md#semantic-navigation-providers).

`explain` takes an editor-style `path:line[:column]`, 1-based, and shows the enclosing
declaration's line with a caret under its name, then the same `●` line `search` prints,
who may name it, `↗` every other address a re-export offers it at, and `←` the files
importing it at any of them:

```console
crates/vvv-core/src/semantics.rs:40:12
    39 │ pub fn reach_kind(&self, modifier: Option<&str>) -> ReachKind {
       │        ^^^^^^^^^^

● method reach_kind   pub
  reaches  everyone
← 5
  crates/vvv-core/src/answer/mod.rs
  …
```

On an import statement it says where the thing really comes from — what the path
spells, then `↗` the declaration a re-export chain leads to and its file. Inside a
grouped import, the entry under the column takes precedence over the enclosing
statement, including in nested groups:

```console
crates/vvv-engine/src/change.rs:12:20
→ vvv_core::ChangeSet
  ↗ vvv_core::edit::change_set::ChangeSet   crates/vvv-core/src/edit/change_set.rs
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

● struct Span   crates/vvv-core/src/text/span.rs:9:12   ← 26
  ↗ vvv_core::text::Span
  ↗ vvv_core::Span
```

`impact` is rings: depth 1 is every module importing the declaration (or an address
that re-exports it), depth 2 every module importing one of those, and so on to depth
8. A module is listed once, at the first depth it is reached, with `via` the module
that carried it there:

```console
$ vvv impact Plan
impact Plan

← 2   depth 1
  src/workspace/mod.rs
  src/lib.rs

← 1   depth 2
  src/engine.rs   via vvv_core::workspace
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

## Gathering focused context

`context` follows an exact occurrence and returns source excerpts with directly
related definitions. It uses the same syntax-based navigation as the definition
pane, with no language-server installation or process.

```console
vvv context src/engine.rs:20:12
vvv context src/engine.rs:20:12 --max-bytes 8192 --max-items 8 --json
vvv context src/engine.rs:20:12 --references --max-files 32 --max-lookups 64
```

Results start with the declaration (including attached documentation), followed by
definitions referenced in its body. JSON carries the nearest enclosing declaration
in `enclosing` as a versioned location, without repeating its body. Add
`--include-enclosing` when you want that body as an additional item. Repeated
targets are included once. Each item carries the relationship, exact source range,
source version, and the occurrence establishing the relationship. `--select`
resolves an ambiguous starting occurrence using the same candidate IDs as `navigate`.

Use `--detail signature` to read attached documentation, attributes, parameters,
return types, and constraints without implementation bodies:

```console
vvv context src/engine.rs:20:12 --detail signature --json
```

`body` remains the default. Signature mode follows only outgoing identifiers inside
the signature, and renders related declarations (and `--include-enclosing`) at the
same detail. TypeScript/TSX variables and class fields with arrow, function,
or generator initializers retain the declaration prefix and callable header while
excluding the initializer's implementation. Arrow signatures retain `=>`; expression
bodies, including JSX, are omitted. Named function-expression bindings also provide
their own callable header. A later binding in a multi-declaration statement starts at
its own declarator, avoiding earlier implementations. Body expansion retains the
indexed declaration extent. Parentheses, non-null assertions, `as`, `satisfies`, and
TypeScript angle assertions around a written callable retain the exact prefix up to
its body. Closing wrappers and assertions after the body are omitted; excerpts are
source prefixes, not reconstructed standalone declarations. Wrapper traversal is
bounded to 128 levels and 1,024 validation nodes. Literals, class expressions,
unknown wrappers, and unfinished callable headers remain unsupported; no runtime callable
inference is performed. Rust and TypeScript use AST body boundaries; braces inside types do
not end a signature. Container signatures are headers, excluding members; tuple
structs and type aliases retain their defining types. Unsupported forms, including
initialized Rust constants and variables, report `signature.outcome: "unsupported"`
with no source text; request `body` to retrieve them. A complete signature item
has `complete: true` even though the implementation is omitted.

In `serve` and MCP, pass `detail: "signature"` to `context_page`/`vvv_context`.
Each signature item provides `body_expansion`: pass it to `expand`/`vvv_expand`
to read the **whole declaration from its beginning**. An item's `expansion` handle
instead continues a signature shortened by the output budget. Both handles are
independent and preserve exact source ranges and versions.

`--references` scans the original spelling plus named import aliases and renamed
re-exports whose bindings resolve to the selected target. Each use is independently
confirmed by navigation before its enclosing declaration is included. Rust scoped
named imports participate. TypeScript and TSX files are scanned together.
A confirmed reference under a `test` or `tests` path component is labeled as such;
this is evidence of a related test location, not proof of test coverage. Unenumerated aliases,
unresolved references, and dynamic calls are not a complete caller graph.

The defaults are 16,384 compact JSON result bytes, 12 items, 64 additional
navigation lookups, and 64 files for incoming references. Limits are deterministic;
JSON escaping counts against the byte budget. Excerpts may be shortened at UTF-8
boundaries, with `complete: false` and the exact returned range. Omission counts
report byte/item/work limits and references that could not be confirmed. Request
an item's versioned target with a larger budget to expand it. The byte budget
excludes the response envelope and human-output styling. It is not a memory limit
or execution deadline. Ambiguous candidates are never silently removed to fit;
an oversized candidate set returns `output_limit`.

`vvv discover --json` lists this build's languages, commands, parameter names, and
context/output limits. It does not scan the workspace. In `serve`, the same command
is `{ "command": "discover" }`; a call may set `max_output_bytes` to limit its JSON
result. An oversized read-only result returns a structured `output_limit` error.
For `context`, this limit also narrows its excerpt budget. Mutation commands reject
this option before running, so an output limit cannot hide a successful write.

## Reviewing and applying retained mutation plans

Use a single `vvv serve` or MCP session when application must use exactly the edits
you reviewed. Ordinary `rename`/`rewrite` previews and their `--apply` invocations still build separate
plans. Retained handles stay in memory; they do not survive a restart or transfer
between sessions/workspaces.

In `vvv serve`, send these requests one at a time, substituting the returned
`plan_id`:

```json
{"command":"prepare_rename","intent":{"name":"Engine","to":"Runtime","declared_in":"src/engine.rs"}}
{"command":"inspect_plan","plan_id":"<returned plan_id>"}
{"command":"apply_plan","plan_id":"<returned plan_id>"}
```

Preparation does not write files. It returns `state: "prepared"` and a complete
`preview` with rename occurrences, confidence, skipped/unresolved cases, file edits,
and diffs. Review these before applying. Increase `max_bytes` (up to 1 MiB) when the
complete preview does not fit; the engine never removes edits to fit a budget.
Inspection returns the captured preview, without replanning or claiming it is fresh.

Application checks the captured source inventory, file contents, manifests, and
walk configuration, then applies the retained edits through the existing transaction
and undo lifecycle. Changed inputs return `stale` and require a new review. This is
conservative: even an unrelated source edit can invalidate a plan. These checks do
not lock out external editors.

A successful result includes the `plan_id`, committed `history_id`, and post-apply
file content identities. Repeating `apply_plan` returns that same historical receipt
without writing again, even after undo or later edits. `inspect_plan` also retrieves
completed receipts and failures. A failed attempt is consumed; inspect its error
and any recovery details before preparing a replacement. Applying runs source and
transaction checks; request project validation separately with `validate_plan`. `vvv history` and `vvv undo` use the existing durable history; undo always
reverses the latest history entry, not an arbitrary plan handle.

`discard_plan` releases a pending plan without writing. Its discarded status is
retryable until a later preparation reclaims it. Successful receipts remain retained
until expiry. Limits are 16 retained plans/outcomes, 16 MiB charged per plan, 64 MiB
combined, and a fixed 10-minute lifetime from preparation. Inspection and retry do
not extend that lifetime. Capacity failure preserves other pending plans and receipts.

MCP provides `vvv_prepare_rename`, `vvv_inspect_plan`, `vvv_apply_plan`, and
`vvv_discard_plan` with the same arguments, omitting `command`. Only `vvv_apply_plan`
can write. Queued calls can be cancelled; once apply starts it finishes its
transaction. If a response is interrupted, inspect or retry the same handle.

### Retained file and directory moves

Use `prepare_move` to retain the existing file/directory move planner's exact edits
and destinations. MCP exposes the same request as `vvv_prepare_move`:

```json
{
  "command": "prepare_move",
  "intent": { "from": "src/origin.rs", "to": "src/relocated.rs" },
  "page": { "max_items": 20, "max_bytes": 8192 }
}
```

Both paths must be nonempty workspace-relative paths with no `..` components.
The existing move rules apply, including companion files, import rewrites, notices,
and destination-preserving writes. Inspect, review, apply, discard, and validate
through the same handle commands as rename. Application does not rerun the planner.
Retained batches are unsupported.

Move validation checks final destination contents, disappearance of old source
entries (allowing case-only aliases), and unchanged inventory outside the reviewed
moves. It also retains ignore configuration along source/destination ancestors,
including recorded absence. These observations remain checked during commands,
even for hidden or ignored destinations. No fresh post-apply tree is adopted as
the reviewed baseline. Receipt retries remain historical after edits or undo.

### Selected and retained symbol moves

`move --symbol NAME` moves one declaration between existing files of the same language.
A unique supported declaration needs no selection. When several declarations match,
vvv lists their row numbers and ids; pass `--select ROW_OR_ID` to choose exactly one.
Selection never substitutes another declaration when the chosen one is unsupported.

```console
vvv move src/origin.rs src/destination.rs --symbol Engine --select 1
```

For agents, `symbol_move_candidates` takes `name` and workspace-relative `from`.
Its reply includes source `content`, all matching addressable declarations in source
order, ids, supported pieces, and optional typed `unsupported` reasons. Use the id
and `content` in preparation:

```json
{
  "command": "prepare_move_symbol",
  "intent": {
    "name": "Engine",
    "from": "src/origin.rs",
    "to": "src/destination.rs",
    "selection": { "ids": ["<declaration-id>"] },
    "expected_content": "<source-content>"
  },
  "page": { "max_items": 20, "max_bytes": 8192 }
}
```

MCP exposes `vvv_symbol_move_candidates` and `vvv_prepare_move_symbol`. Apply,
inspect, review, discard, and validate use existing handle commands. Application
uses the captured declaration and edits; receipt retries remain historical.

Declarations retain documentation, attributes, and supported export wrappers. Rust
impl blocks travel only when syntax establishes their target in the declaration's
scope, including generic and trait impls. Module declarations (use a file move instead), standalone signatures and variable declarators, nested declarations, competing same-module
bindings (including conditional declarations and overloads), missing ownership evidence,
and ambiguous or unsupported companion targets are refused. Qualified targets and
cross-scope targets without a local owner are conservative unsupported cases. A
destination binding with the same name and moving into the source file are refused.
TypeScript overload signatures appear as separate function declarations in search
and candidate discovery. No type inference or conditional compilation decides ownership.

### Retained rewrites and paged reviews

`prepare_rewrite` retains the existing rewrite intent: `query`, `template`, and
optional `selection`. MCP exposes it as `vvv_prepare_rewrite`. Captures expand
from the captured source; application cannot replace the template or selection.
Use the same inspection, apply, discard, and validation commands as for rename.

For a large rename, rewrite, file move, or symbol move, supply `page` during preparation or inspection:

```json
{"command":"prepare_rewrite","intent":{"query":{"pattern":"increment($X, 1)"},"template":"increment($X, 2)"},"page":{"max_items":20,"max_bytes":8192}}
{"command":"review_plan","cursor":"<next_cursor>","page":{"max_items":20,"max_bytes":8192}}
```

The MCP continuation tool is `vvv_review_plan`. Follow `next_cursor` until null
before applying. Pages contain ordered metadata and exact text chunks, including
large replacements and single-file diffs. Reassemble chunks by section, index,
optional file index, JSON-pointer field, and UTF-8 byte offset. Nothing is silently
omitted. The first page includes the complete intent; an unusually large template
or selection may require a larger first-page budget. Full review remains the
fallback when `page` is omitted. See [the record contract](protocol.md#paged-plan-reviews)
for reconstruction details.

Review cursors read captured evidence without accessing the current source tree.
Edits do not change their content; applying still checks freshness. Retries with
the same budgets return the same page. Changing budgets resumes from the same
position. Captured reviews remain readable after apply or failure, until the
original expiry. Discard releases a pending review and its cursors. Pagination
and validation do not extend expiry or bypass retention limits. Fetching every
page is a client review responsibility, not an enforced apply prerequisite.

## Validating an applied change

After `apply_plan`, ask the same session to run explicit check commands:

```json
{
  "command": "validate_plan",
  "plan_id": "<returned plan_id>",
  "checks": [
    {
      "name": "format",
      "program": "cargo",
      "args": ["fmt", "--all", "--check"]
    },
    { "name": "compile", "program": "cargo", "args": ["check", "--workspace"] },
    {
      "name": "targeted tests",
      "program": "cargo",
      "args": ["test", "-p", "my-library", "engine::tests"]
    }
  ],
  "extra_inputs": [".cargo/config.toml"],
  "budget": { "timeout_ms": 120000, "max_bytes": 16384 }
}
```

Replace the commands and test target with your project's checks. Omit
`extra_inputs` if there are no additional hidden/ignored files to include; every
listed file must exist. MCP exposes the same request as `vvv_validate_plan`.
`discover.validation_available` reports availability: supported Unix and Windows disk workspaces execute checks; virtual workspaces
do not execute validation programs.

Commands run in order from the workspace root with no implicit shell and no stdin.
On Windows, programs must be native `.exe` executables; extensionless names resolve
as `.exe` through the workspace root and `PATH`. Batch files require an explicitly
named shell program, such as `cmd.exe`, with caller-supplied arguments.
They inherit the session's environment and permissions and **are not sandboxed**:
compilers, build scripts, and tests can write files or access the network. Use a
formatter's check mode. vvv does not choose or install commands, format code
automatically, or roll back their effects. MCP marks this tool as able to write
and access external resources.

Each result records the exact command, outcome, exit code, elapsed time, and separate
bounded stdout/stderr. `passed` requires all checks to succeed and the observed
inputs to remain unchanged. Failures do not undo the applied plan. `inspect_plan`
retains the latest validation under `validation`, while the apply receipt stays
unchanged. Another validation request reruns the commands and increments `run`.
Cancellation and timeout stop the active process group; after an interrupted
response, inspect the plan. Cleanup also stops remaining group members after normal
command exit. Checks not started remain `not_run`.

Validation rejects source or inventory changes since apply before starting. It
records digests of workspace-visible files, including binary resources, the
planner's source/configuration inputs, and explicit `extra_inputs`. Source changes
during the checks prevent a pass and stop later checks. Ignored build outputs and
unlisted hidden files, installed dependencies, tools, environment variables, and
external services are outside that evidence. Before/after observations cannot detect
a transient change that is reverted between captures. These results are recorded
evidence for the observed inputs, not a sandbox or a lock on the workspace.

At most four commands run per request. The default batch deadline is 60 seconds,
up to 5 minutes; input capture before/after the batch and process cleanup can add
latency. Output defaults to 16 KiB, configurable from 4 KiB to 1 MiB. Logs can be
truncated; statuses and input identities are retained. Inspecting a large recorded
report may require increasing both `max_bytes` and MCP's `max_output_bytes`.
Validation uses the plan's original ten-minute expiry and does not extend it.

## Paging results in an agent session

For a large search, context, or relationship request, keep `vvv serve` open and ask for pages:

```json
{"command":"search_page","query":{"pattern":"Engine"},"page":{"max_items":20,"max_bytes":8192}}
{"command":"continue","cursor":"<next_cursor>","page":{"max_items":20,"max_bytes":8192}}
```

Copy `result.next_cursor` into the next request until it is absent or null. Matches retain
their IDs and original one-based ordinals. A match too large for a page produces
`output_limit` with the required size rather than a shortened match.

Use `context_page` with the same origin/selection as `context`, separate `page`
and `work` budgets, and optional `references: true` or `include_enclosing: true`. Continue its query cursor for
more relationships. For a shortened definition, use its separate `expansion` token:

```json
{ "command": "expand", "cursor": "<item.expansion>", "max_bytes": 4096 }
```

Append each chunk's exact text until `done` is true. Relationship traversal and
excerpt expansion proceed independently. A short or empty context page can mean
work found no new declaration; follow its cursor until traversal is complete.

Cursors belong to this session and can be retried. Edits invalidate them, including
changes in files that did not match earlier. On `stale` or `cursor_expired`, restart
the original query using a current source position. The engine retains at most
16 queries for ten minutes, subject to byte limits published by `discover`.
Pagination bounds result delivery and retained state; each request still verifies
the workspace contents. See [the protocol](protocol.md#paged-queries-and-exact-source-expansion)
for request shapes, limits, and structured recovery actions.

## Inspecting API schemas

The default build includes offline JSON Schemas for every command. Discovery adds
`schemas_available` and, when enabled, schema identifiers on each command entry.
Retrieve a contract without scanning source files:

```console
vvv schema context --contract arguments
vvv schema navigate --contract response --json
vvv schema --contract call
vvv schema --contract reply
```

`arguments` describes command-specific inputs, `request` includes the `command`
field, `result` describes a successful payload, and `response` includes success
and error envelopes. `call` and `reply` describe the whole session protocol and
take no command name. Human output prints the schema document; `--json` returns
the usual vvv envelope containing its `id` and `document`.

The `schemas` CLI feature is enabled by default. For a minimal build, add it
explicitly with `--no-default-features --features rust,schemas`. Without it,
discovery reports `schemas_available: false` and the `schema` command is absent.
Engine/library consumers opt into the engine's `schema` feature independently.

The generated [schema artifacts](schemas/v1/) include their definitions, so a
client can validate requests and responses without network access. Schema shape
validation does not resolve symbols, check source versions, or replace engine
validation of paths, ranges, and query meaning. See the
[schema protocol](protocol.md#vvv-schema) for identifiers and regeneration.

## Choosing what a command acts on

Every row in a search is numbered, and `--select` takes those numbers — single rows,
ranges, or both:

```console
vvv search 'foo($$$A)'                                  # look at the rows
vvv rewrite 'foo($$$A)' 'bar($$$A)' --select 2,5-7 --apply
```

Numbers select positions in the current search result order. Separate CLI
invocations repeat the search; source changes can change which match an ordinal
selects. `--json` carries a content-derived `id` per match (`827aff1a882c`, computed
from its path, byte span, and matched text). `--select` also accepts these IDs and
rejects IDs absent from the current results. Use IDs when retaining selections
across invocations. A plan's fingerprint protects its witnessed source snapshots
between planning and writing. Without `--select`, a command uses its default
selection: rename uses the confidence rules described below, and rewrite selects
all matches.

## Previewing, applying and undoing

None of `rewrite`, `rename` or `move` writes anything on its own. Each one works out a
plan, prints it, and stops; the last line on standard error is the plan's size and the
flag to go on with (`± 2   2 files` / `hint: --apply to write`). When you add
`--apply`, that invocation computes a plan, checks it, and writes it. A separate
preview invocation does not retain a plan for the next CLI invocation. Rust clients
can retain a `Planned<T>` and pass it to `Apply`; apply rejects it if a witnessed
source has changed. The same check rejects a source changed after the engine read
it during planning, including a cached source in a session. Plans witness the files they edit
or move; files consulted only during resolution are not re-checked at apply.
What you get back is a receipt naming the history entry: `✓ #3   ± 2   2 files`.

Apply and batch keep their recovery state until the history save succeeds. A file
operation or history-save failure restores the pre-apply file contents, locations,
case spelling, ledger bytes (or absence), and owned empty directories, including
the file or ledger whose write failed partway through. If restoration cannot finish
or be verified, `recovery_failed` names the remaining effects and any paths whose
state is unknown. Recovery is in memory: interruption or a crash has no automatic
recovery. It does not provide isolation from external writers or restore inode
identity, timestamps, or complete filesystem metadata.

File and symbol moves follow resolved qualified paths through imported module
aliases. An alias path keeps its spelling when changing the alias import is enough;
otherwise the path is rewritten to the moved target. Grouped entries retain their
group when its resolved prefix still covers the target, including module aliases. Imports needed by a moved
declaration and redundant destination imports use the same resolved edges.

Invalid declaration extents or overlapping edits in a symbol move are rejected as
a conflict before any write. Paths inside moving declarations and their companion
pieces are transformed once and spelled from their destination; self references
continue to name the moved declaration, while references to siblings left behind
continue to name those siblings.

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

Successful undo restores the receipt's file contents, locations, and case spelling,
removes its history entry, and removes owned empty directories (a moved file shows
as `src/b.rs → src/a.rs`). Pre-existing directories, directories containing other
files, and directories without receipt ownership evidence are retained.
If you've edited one of those files since the apply, undo refuses before restoration
and leaves the history entry in place.

Undo keeps its recovery state until the updated history ledger is saved. If file
restoration, directory cleanup, or that history save returns an error, undo restores
the pre-undo file and ledger state, including directories it removed, or returns
`recovery_failed` naming confirmed remaining effects and unverified paths.
Apply, batch, and undo retain recovery state for returned file-operation,
directory-cleanup, and history-save errors. The guarantee covers returned errors,
not panics. There is no durable journal or crash recovery, and commands do not
isolate concurrent writers. Restoration covers file contents, locations, case
spelling, ledger bytes, and owned empty directories; it excludes inode identity,
timestamps, and complete filesystem metadata.

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

● enum Reach   crates/vvv-core/src/semantics.rs:105:1

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

Imported aliases are followed through chains, irrespective of import order.
Rust module-level bindings can be followed across files, including
`use crate::a as parent` in a parent and `use super::parent as local` in its child.
Dependencies, explanations, navigation and references share the underlying targets.
Private bindings are available to their module and descendants; `pub`, `pub(crate)`,
`pub(super)` and `pub(in path)` constrain cross-file lookup. Function-local and
inline-module imports are not promoted to bindings of the containing file.
Competing targets remain separate navigation candidates; dependency paths with
multiple targets have no single resolved address. Unseeded import cycles stay
unresolved. Moving an alias's target preserves client spellings when updating the
binding supplies the required target.

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

Every destination must be absent, except a case-only file rename on a
case-insensitive filesystem: `config.rs` → `Config.rs` addresses the same entry
and is allowed. Distinct hard links still count as occupied destinations.
Apply checks all move destinations again before
writing, so a file created after planning is preserved and the move is refused.
The move itself also refuses to replace a destination created after that check,
and recovery moves protect occupied destinations too. Disk moves use native
no-replace renames where supported; the unsupported-operation fallback links then
removes the source. That fallback is destination-preserving but not atomic: if
source removal fails, recovery removes the acquired link or reports it as remaining.
Neither path falls back to copying across filesystems.
Case-only renames use a unique temporary name and two destination-preserving moves;
they are not atomic as a whole. Recovery and undo restore the original stored
filename as well as its contents. A recovery failure names any temporary file left
behind. A file created between the two legs is preserved.
Preflight does not reserve the paths; destination-preserving move operations enforce
the no-replacement requirement at each move.

The preview separates what is mechanical from what you should read:

```console
$ vvv move crates/vvv-core/src/semantics.rs crates/vvv-core/src/resolve/semantics.rs
move crates/vvv-core/src/semantics.rs → crates/vvv-core/src/resolve/semantics.rs

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

Every path re-spelled in place is one `→`
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
moves a uniquely supported declaration called `Config` — with its doc comments,
attributes, and proven Rust impl companions — to the end of an existing file of the
same language. Multiple candidates require `--select`; see
[selected symbol moves](#selected-and-retained-symbol-moves) for support limits.
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
select the next/previous result file without leaving the query, restoring that
file's selected match and updating its previews. A `?` in a pattern (a Rust `?`, a
TypeScript `x?: T`) is just text. `esc` goes back one step, ultimately to search;
`e` opens `$EDITOR` at the row under the cursor; `?` on a list or detail shows the
marks and every key that works where you are, by where it comes from (`j`/`k` scroll
it; Page Up/Down and Home/End also work; another key closes it). **F1** opens
contextual help from every pane, including inputs and overlays, and returns to
the same place when pressed again. Help wraps descriptions and lists only keys
that are active at that focus, excluding shadowed shortcuts. The footer keeps
F1 visible and shows complete hints that fit.
The status bar and that list are two views of one table, so they never disagree. The picker keeps what it has read between keystrokes and re-reads only files that changed;
it looks at the tree again at most once a second while you type, and always right
after the editor returns or it writes something itself.

**Search** is the hub. The left column separates files from the selected file's
matches. The lists share a border; the file list uses a bounded part of the column
and shrinks when there are few files, leaving the remaining rows for matches.
Files show full workspace-relative paths and match counts, with filenames
emphasized in place. Long paths wrap in reading order at path separators and
remain visible while matches scroll. Each match shows its line number and source
excerpt, with the hit highlighted. The line-number column fits the largest match
line in the selected file and stays aligned while scrolling. Long excerpts retain the hit and surrounding
code, marking omitted sections with ellipses. Files and Matches have independent
focus and scrolling. From a non-input pane, `1` focuses Query, `2` Files, `3`
Matches, `4` Source, and `5` Definition; Tab/Shift+Tab follow that order, skipping
unavailable panes without renumbering the shortcuts. Relations without a file
list skip Files. Query Enter jumps directly to Matches.

In Files, `j`/`k`, arrows, or Ctrl+N/P select a file and immediately update Matches and the
preview. Enter or Right/`l` focuses Matches; Escape focuses Query. In Matches, movement stays
inside the selected file, including at its first and last occurrence. Escape
focuses Files. Page Up/Down move by a visible page, and Home/End or `g`/`G` select
the first/last item in the focused list. `[`/`]` switch files directly from
Matches. Returning to a file restores its last selected match and list viewport.
Focused panes have a stronger border and a deep teal selection background with a
bright mint marker. Unfocused selections retain a subtler teal background and
marker at normal text brightness. Selection spans the full row, including wrapped
paths, while preserving syntax and hit colors. With color disabled, the focused
selection uses reverse video and retained selections keep their markers. Counts and arrow indicators show position and scrollable
content.

Files and Matches share the search shortcuts: `f` location, `s` symbol kind,
`t` category, `L` language, `R` relation in an entered view, and `/` or `i` query.
Actions such as follow, rename, move, and rewrite use the selected match shown
in Matches, including when Files has focus. While editing the file filter,
ordinary characters remain text.

Click a file or match to select it and focus its pane; wrapped path lines select
the same file. Clicking a title or blank space only changes focus. The mouse
wheel scrolls the pane under the pointer by three displayed rows without changing
selection or keyboard focus. Keyboard movement reveals the selected row again.
Mouse input does not pass through overlays. Mouse reporting is suspended while
an editor owns the terminal. On short terminals, the focused list receives the
available browsing space; the other list remains reachable through its header
and keyboard focus.

From a non-input panel, `F` edits an inline fuzzy filter on result file paths.
Matching is case-insensitive and accepts subsequences such as `ctx` or `cli srv`;
every space-separated term must match. Filename, boundary and consecutive matches
rank higher, and matching path characters are highlighted. Typing updates the
file and match lists locally, with no engine search. Arrows or Ctrl+N/P switch
matching files while editing; Enter keeps the filter and returns to matches,
Escape restores the previous filter, selection, list viewports, and focus. Tab or
clicking another pane accepts the filter before changing focus. Ctrl+U clears the
filter both while editing and when Files has focus.
The filter remains visible after editing. Empty results show explicit feedback.
Categories and location scope still apply before file filtering.

Pane borders hold the active location, symbol kind and language, result counts,
category tabs, and source locations. Counted labels use singular wording for one
result and plural wording for zero or multiple results. `t` chooses **All**, **Declarations**,
**Imports**, or **Uses**; the border abbreviates labels on smaller terminals.
Categories filter search occurrences, not resolved relationships. `s` chooses a
symbol kind and switches to Declarations; choosing another category clears that
kind filter. `L` chooses a language. These pickers accept text to filter their
choices, arrows or Ctrl+N/P to move, Enter to select, and Escape to cancel. Filter
words (`symbol:trait`, `name:Foo`, `kind:impl_item`, `lang:rust`) still work in the
query. Ctrl+F, Ctrl+S, Ctrl+L and Ctrl+T open location, kind, language and category pickers while
query focus keeps ordinary letters as text.

The search border groups active location, symbol kind, language, node kind,
category, and fuzzy file restrictions. **Ctrl+G** opens their shared menu from any
search pane: Enter edits the selected filter (node kind returns to the query),
**Ctrl+X** clears it, and **Reset all filters** keeps the name or pattern being
searched. The direct picker shortcuts still work. Inside a picker, **Ctrl+U**
clears its text, **Ctrl+X** removes that picker’s restriction, and Escape cancels
without changing the search. Clearing symbol kind also restores all categories.

Picker borders label counts as **loaded hits**: they describe the last completed
search, before category and fuzzy file filtering, within the current location.
Location counts describe paths within that loaded search; sibling directories
may have more matches when searched. Zero-count choices remain selectable, and
counts are omitted while a search is pending or stale. A check marks the applied
choice independently of the cursor. Long choices and active restriction paths
wrap rather than hiding their filenames.

Empty results identify the restrictions to adjust. Filter changes retain focus
and the chosen preview pane. A match hidden by a restriction is remembered and
restored when clearing makes it visible again; deliberate file or match navigation
establishes a new selection instead. Query edits start a new search selection.

`f` chooses where results are located. The picker suggests directories observed
in search results and accepts a typed workspace-relative file or directory path;
**Entire workspace** removes the restriction. Location filtering uses the engine's
component-aware search scope: `crates/vvv` does not include `crates/vvv-lang`.
Definitions resolve across the whole workspace. A use in `crates/vvv` can therefore
preview and follow a declaration in another crate; its definition border marks
**outside location**. Location and category are restored by browsing Back. Within
an entered declaration's references, location filters the judged occurrence list;
leaving it reruns search if its location changed.

The results list is where you act: `↓` or `⏎` from the query (from a preview, `esc`), then `r` to rename, `m` to move the file, `M` to move the declaration,
`w` to rewrite the category's search matches, `h` for history, and `u` to undo the
newest apply. `v` switches references and other reports between compact rows and
the full report's result rows; ordinary search keeps source excerpts in both views.
The footer prioritizes navigation; `?` or F1 lists the available actions.
CLI flag hints stay in the CLI.

Inside an entered reference view, rewrite still uses the retained search matches;
reference location and verdict filters do not change that search. Rename and move
continue to plan against the declaration and its workspace-wide consumers.
The fuzzy file filter only changes navigation, including within references; it
never narrows rewrite selection or changes an operation's scope. Reference files
appear once, with each match marked by its confidence and global verdict counts
retained on the border. Following a declaration clears the file filter on the
destination page; browsing Back restores it along with file and match selection.

A **definition preview** shows the selected occurrence's resolved declaration,
including its signature and body, without line numbers or a gutter. Wide
terminals stack source and definition in fixed panes in the right column, leaving
the full left column for results. Selecting a declaration, loading a preview, or
receiving empty results keeps those panes in place. Below 110 columns or at short
heights, one preview fills the right column. From a non-input panel, `4`
focuses source and `5` focuses definition, revealing the chosen preview when
only one fits. `p`
toggles between the previews and focuses the destination. Source is shown by
default when only one preview fits. The chosen preview persists when returning
to results or the query, selecting another file or match, filtering, and resizing.
Explicitly following a reference opens its definition; browsing Back restores the
previous preview choice. Both previews keep independent
scroll positions. Source locations sit beside preview titles on the top border;
when they cannot fit, the complete path wraps directly beneath the title.
Resolution status appears on the definition pane's bottom border.
When selecting another file, the source pane keeps its previous text, location,
highlight, and scroll position while the replacement loads. An `updating` hint
marks this interval; the text and its location switch together when the selected
file arrives. Replies for files no longer selected cannot replace the pane.
Revisiting a file reuses its parsed preview when a fresh read confirms unchanged
contents. The cache is bounded; large or evicted files parse again. Returning from
the editor marks displayed source as stale. Refresh (`Ctrl+R`) and search results
identifying changed contents reload the selected source, even if its path did not
change.

The declaration's outer indentation is removed; indentation within its body is
preserved and stays fixed while scrolling. Code uses a fixed left inset.
Selecting an enum variant previews the containing enum and highlights the
variant's name; `e` opens the selected variant's declaration line.

The engine resolves the exact source occurrence. A same-named struct elsewhere or
an enum variant does not hide a definition reached through imports and re-exports.
Several possible targets show “Several definitions match”; unsupported contexts
show “Cannot follow this reference yet”. Other outcomes distinguish a missing
identifier, unresolved name, external source, or cyclic imports. These are the same
outcomes exposed by `navigate`.

Preview space remains reserved while navigating or loading. The previous
definition and scroll remain visible until replacement source and metadata arrive
together, with **updating** on the border. Replies for an older selection cannot
replace the current request.

`5` focuses the definition; `tab`/`shift-tab` include it in the panel cycle and
reveal it when sharing a single preview with source. Use `j`/`k` or arrows to
scroll, `d`/`u` or Page Down/Up to page, and `g`/`G` or Home/End to reach the
top/bottom. Left/Right or `h`/`l` scroll code by eight terminal columns; `0`
restores the first column. Source line numbers stay fixed. The border shows the
visible file-line range and horizontal offset. `esc` returns to results; `e` opens
the selected occurrence or displayed declaration unless find or go-to-line has
chosen a specific line.
Moving between uses of the same definition preserves
scroll. A different definition or source version resets it; selecting a variant
outside the visible portion of its enum reveals that variant.

**Inspecting code.** In either preview, `/` finds literal, case-sensitive text
within its displayed source: the whole file in Source, the enclosing declaration
in Definition. Matches highlight as you type; Enter keeps the find and `n`/`N`
cycle hits, wrapping at either end. The bottom border shows the term and hit
position. Escape during editing restores the previous find and scroll position;
Ctrl+U clears the text. `:` accepts an absolute file line, with the valid range
shown for an invalid entry. Enter jumps there; Escape cancels. `e` then opens the
found or requested line in the editor. Source and Definition retain independent
find terms, hit positions and horizontal scroll. Replacement files reindex the
find against the displayed bytes.

`z` expands the focused preview across the area below Query; `z` or Escape
restores the split layout without changing focus, selection or list positions.
`p` switches previews while expanded. Focusing Query, Files or Matches restores
the layout. Use `i` to edit the query from a preview; `/` searches its code.
Browsing Back/Forward also restores inspection and expansion state.

**Following code.** Press `o` on a result to follow its exact occurrence to a
definition. In the context or definition pane, Enter or `o` opens an identifier
picker. Type to filter, use arrows or Page Up/Down to choose, and Enter to follow.
Each occurrence shows its line, column, and surrounding source, so two uses of the
same name remain separate choices. If several definitions match, choose one from
a second picker showing kind and location. Escape cancels without changing pages.

**Ctrl+O** opens **Places** from any search pane. Its border has two tabs:
**Trail**, the current session’s browsing pages, and **Recent**, up to 24 completed
search configurations. Tab/Shift+Tab switch tabs; typing filters queries and
complete paths (all space-separated terms must match). Arrows or Ctrl+N/P select;
Page Up/Down and Home/End navigate the list; Enter opens the selection. Escape
cancels without moving the underlying selection or scroll positions. Ctrl+U
clears the picker text, and F1 explains its controls.

The trail marks the current page and supports jumping directly to any retained
page. Its entries show the page, source location, and active restrictions, wrapping
complete paths without blank gaps. Editing a completed query or changing filters
retains the page being left; typing while a search is pending does not create a
page per keystroke. The search border shows the current trail position and which
Back/Forward directions are available. Returning restores saved selection, focus,
filters, preview choice and scroll, then validates the destination’s source.

Recent searches retain query text (including language, symbol and node filters),
location, category and fuzzy file filter. Reopening runs a fresh engine search;
restrictions appear on the usual borders. **Ctrl+D** in Recent forgets its selected
entry; a new completed search can record that configuration again.
**Ctrl+L** in Places resets split width, report view and preview choice.

On normal exit, the picker saves these layout choices and recent searches per
canonical workspace. Startup restores the layout and makes the recipes available
in Places; it starts with an empty query and unrestricted scope. Source previews,
browsing pages and mutation plans are never saved to disk. State lives in
`$XDG_STATE_HOME/vvv/tui` when set, otherwise `~/Library/Application Support/vvv/tui`
on macOS, `%LOCALAPPDATA%/vvv/tui` on Windows, or `~/.local/state/vvv/tui` on other
systems. Invalid, oversized or incompatible state is ignored.

A successful follow opens the declaration and focuses its definition pane.
Alt+Left and Alt+Right restore previous/next browsing locations, including the
query, selected row, focus, and both scroll positions. Row previews, failures, and
cancelled choices do not add history. A successful follow after going Back replaces
the forward branch. Navigation history is separate from mutation/undo history.

Restored pages retain their captured source while the engine validates the saved
occurrence and target. A changed or missing definition leaves the page marked
stale; Ctrl+R reruns its retained search so you can select a fresh occurrence.
Returning from the editor also marks the page stale. Following stale anchors is
blocked. The trail retains at most 64 pages with a 16 MiB estimated payload budget;
shared payloads are charged per entry, and the oldest entries are evicted first.
The currently displayed page is outside this retention budget.

`⏎` on a declaration — or on an occurrence whose exact navigation has resolved
one definition — _enters its references_:
the rows become that declaration's judged references, grouped by verdict (`✓ safe`,
`? unverified`, `✗ another declaration's`), each with the reason's glyph. A name
search becomes the symbol's impact and context. `r`, `m` and `M` then act on the
entered declaration from any row, and `esc` leaves the scope, then the query. `R` opens the relation menu:
all references, one verdict, `impact` (the modules importing it, depth by depth),
`definition` (its address, reach and importers) or `deps` (the declaring file's
imports and who imports it). Each answer is written by the engine, not guessed.

**Reviewing operations.** Rename, move and rewrite share numbered pane titles,
full file paths and compact source rows. Complete paths appear once above each
consecutive file group, wrapping when necessary; source line numbers size their
gutter to the list. The active list receives the remaining height, with compact
sibling previews or collapsed borders in a short terminal. Tab cycles panels;
empty verdict lists are skipped. Digits jump to the pane number from a non-input
panel. The detail pane keeps following the last list used, including when editing
an input. Home/End in a detail pane scroll its content without changing selection.

Headers show selection/file counts and whether a preview is updating, ready,
invalid, empty or applying. Enter applies only when the preview is ready. Ctrl+U in
an operation input clears its text and preview. Checkbox changes in rename and
rewrite request a new preview for the exact selected IDs; older replies cannot
restore a discarded preview. Inputs and checkboxes stay fixed during apply.

**Rename** starts with an empty new-name field. Its verdict panes are
`2 ? Unverified`, `3 ✓ Safe` and `4 ✗ Other`. Each site has a checkbox (`▪`
selected, `▫` excluded); Space flips one and moves to the next site, and `a`
flips every site in that verdict. Defaults come from the engine's judged plan.
`5 Diff` shows the selected file's planned hunks and the occurrence's full
resolution reason. A site excluded from the plan shows Source instead, with its
exclusion on the bottom border. Enter writes the selected sites.

**Move** edits the destination in `1 Move file` or `1 Move declaration`, with the
complete origin path on the bottom border. Its lists are `2 Paths rewritten`,
`3 Structure` and `4 Manual fixes`. The header counts affected files and manual
fixes. `5 Source` shows a respelling's old/new paths and source; `d` switches to
its diff. Structural changes show Diff, and manual fixes show the full explanation
and action required. Manual fixes are excluded from automatic writes. Enter applies
the complete move plan.

**Rewrite** keeps the search as its pattern and edits a replacement template.
`2 Matches` has the same selection controls as rename; `3 Diff` shows the selected
preview's hunks and any capture values. An excluded site is marked on its border.
Enter writes the selected matches.

**History** lists applied operations oldest first. `1 Entries` marks the selected
operation; its bottom border shows its age and whether it can be undone. `2 Files`
shows the full operation and complete file paths, including move destinations
without repeating the moved file. Enter or `u` requests confirmation to undo the
newest entry. Older entries remain browsable and explicitly say they cannot be undone.

After an apply the picker returns to search, re-runs the query so the rows show the
new state, and reports the receipt (`✓ #3  rename Reach → Scope`) in the status bar.
The apply's report is on screen as a box: `j`/`k` walk its source rows, `e` opens
the row under the cursor in `$EDITOR`, and any other key closes it. Declaration and
import rows retain their source locations. Suggested imports, summaries, and
separators are skipped by the source cursor. In a patch, only lines present in the
current source can be opened: old-side lines for a preview, new-side lines after
apply.

## Driving vvv from a program

`--json` on any command prints one JSON document (see [protocol.md](protocol.md)).
For many commands in a row — an agent, an editor — `vvv serve` keeps one session:
send `{"command": "rename", "name": "Config", "to": "Settings", "apply": true}` on
a line of stdin, read one line of stdout back, and the tree is not re-read between
requests where it did not change. History, undo, and applying an already retained
plan do not refresh the source tree. After a write attempt, the next tree query
checks for changes; a session reuses unchanged file contents. Every command has the same fields as its flags;
errors come back with a `code` to branch on and a `hint` to show.

## Building with fewer languages

Language support is compiled in. The default build includes everything. If you only
want Rust:

```console
cargo install --path crates/vvv --no-default-features --features rust,tui
```

## Connecting an AI client with MCP

Install the optional adapter with `cargo install vvv-rs --features mcp`, or use a
release binary. Default Cargo builds exclude MCP. Start one stdio session for a
fixed workspace:

```console
vvv -C /absolute/path/to/repo mcp
```

For clients that use an `mcpServers` configuration, add:

```json
{
  "mcpServers": {
    "vvv": {
      "command": "vvv",
      "args": ["-C", "/absolute/path/to/repo", "mcp"]
    }
  }
}
```

The client must support MCP `2025-06-18` or `2025-11-25`. Navigation tools are read-only:
`vvv_discover`, `vvv_search`, `vvv_navigate`, `vvv_relationships`, `vvv_context`,
`vvv_continue`, `vvv_expand`, and `vvv_symbol_move_candidates`. Reviewed changes use `vvv_prepare_rename`, `vvv_prepare_rewrite`, `vvv_prepare_move`, `vvv_prepare_move_symbol`, `vvv_review_plan`,
`vvv_inspect_plan`, `vvv_discard_plan`, and the writing tool `vvv_apply_plan`.
`vvv_validate_plan` runs explicitly supplied project checks after apply.
Their input and output schemas and mutation annotations are available through
`tools/list`. Discovery describes the full engine catalog; `tools/list` is the
MCP allowlist.

For example, call `vvv_search` with
`{"query":{"name":"Engine"},"scope":{"packages":["vvv-engine"],"paths":["crates/vvv-engine/src"]}}`.
For a declaration match, form an occurrence anchor from its `path`, `content`, and
`symbol.name_span`; pass it as `{"kind":"occurrence","anchor":...}` to navigation
or context. A declaration's `start` can point at a keyword rather than its name.
For identifier pattern matches, its `span` is already the identifier range.
You can also request context at a zero-based position:

```json
{
  "origin": {
    "kind": "position",
    "path": "src/engine.rs",
    "position": { "line": 19, "column": 11 }
  },
  "page": { "max_items": 8, "max_bytes": 8192 }
}
```

Context's `next_cursor`
continues related declarations through `vvv_continue`; each item's `expansion`
handle retrieves more exact source text through `vvv_expand`. Handles belong to
this session. When an edit makes one stale, restart the original query. Navigation
can return multiple candidates or an unavailable outcome; inspect that outcome
before proceeding. Navigation returns compact locations and evidence; use context
for source excerpts. Very large candidate sets can still exceed the output budget.

Read tools default to a 16 KiB engine result budget (32 KiB for discovery),
configurable with `max_output_bytes` up to 1 MiB. Apply does not accept output
budgets; its compact receipt is checked against a 1 MiB maximum before writes. Protocol framing has separate limits described in
[the MCP contract](protocol.md#mcp-stdio-adapter). Cancellation removes queued work
or stops active read work at the next engine checkpoint; a parser invocation or
pending filesystem operation is not forcibly interrupted. Active apply finishes
its transaction. Closing stdin cancels queued/read work and ends the session once
any active apply completes. Source resolution uses the same engine and ast-grep plugins as
the CLI; no language server is launched.

## Restricting search to paths and packages

```console
vvv search --name Engine --path crates/vvv-engine/src
vvv search --name Engine --package vvv-engine
vvv search Engine --path crates/vvv-engine/src --package vvv_engine
```

`--path` accepts an exact workspace-relative file or a directory prefix, matching
path components: `src/a` includes `src/a/mod.rs`, but not `src/ab.rs`. Use `/`
separators; absolute paths and `..` components are rejected. `.` means the workspace.
These are literal prefixes, not globs. Repeat either flag for alternatives; when
both kinds of filter are present, a file must match both.

`--package` accepts a manifest name or the canonical ID used in vvv addresses
(`vvv-engine` or `vvv_engine` for this repository's engine). Ownership uses the
nearest enclosing package root, so a nested fixture package is not included with
its parent. Files without a known owning package do not match a package filter.
Unknown package names return no matches.

For `search`, `search_page`, or MCP `vvv_search`, pass the same filters as
`"scope":{"paths":["crates/vvv-engine/src"],"packages":["vvv-engine"]}`. The
scope is echoed when nonempty and is retained through continuation. Ordinals and
totals describe only the filtered results. Filtering happens before match
collection; it does not narrow the workspace snapshot used for stale detection.

## Call and reference relationships

Use an exact source position to inspect a symbol's relationships:

```console
vvv relationships callers src/worker.rs:12:8
vvv relationships callees src/worker.rs:12:8
vvv relationships references src/worker.rs:12:8 --path src --package my-package
```

`callers` finds candidate call sites pointing into the selected symbol; `callees`
examines calls owned directly by the selected named function or method.
`references` also includes imports and non-call uses. Named import aliases and
renamed re-exports are followed through the same resolver as navigation. For
example, `use crate::worker::process as execute; execute()` can produce a confirmed
call to `process`, with the `execute` occurrence as evidence.

Confirmed results carry a target and resolution evidence. Ambiguous sites retain
all candidates. Receiver calls such as `engine.run()`, unsupported binding scopes,
and unknown names remain explicit unresolved sites. Calling a function pointer or
callback parameter does not identify its runtime target; it is reported as indirect
when the binding is known. A closure's calls are not attributed to its enclosing
named function. Rust and TypeScript call expressions are supported; constructors,
macro expansion, inferred receiver types, and indirect target analysis are outside
this contract.

Incoming scans examine compatible parser plugins (including TypeScript and TSX),
the selected name, and aliases whose import bindings
resolve to it (or include it among ambiguous targets). Unresolved import probes
are counted in `coverage.unresolved_imports` (or context
`omissions.unresolved_imports` / page `unresolved.unresolved_imports`). This finds renamed named imports without treating every same-spelled token as
a confirmed relationship. It does not enumerate every alias: renamed members reached
through namespace or wildcard imports and aliases assigned through variables may
be missed. Unresolved incoming sites are possibilities, not assertions that the
selected symbol is used there.

`--path` and `--package` filter the scanned files using the same semantics as search.
They do not restrict where a referenced definition may resolve. Increase
`--max-files`, `--max-lookups`, `--max-items`, or `--max-bytes` when needed; defaults
are 128 files, 1,024 lookups, 64 sites, and 16,384 compact JSON result bytes. Inspect
`coverage` in `--json` output: `scan_complete` describes the candidate scan only,
while `limitations` records the remaining analysis gaps. An unfinished scan returns
`next_cursor`. In `serve`, MCP, or a retained library engine, pass it to
`continue`/`vvv_continue` until absent, including after empty pages. Continuation
uses `page` and `work` budgets and preserves the original subject, kind, scope,
alias discovery, and undelivered sites. Retries are non-consuming; changed source,
manifests, or inventory require restarting. A standalone CLI invocation ends its
session, so increase budgets or narrow its scope, or keep `serve` open to continue.

MCP exposes this as `vvv_relationships`, with `kind` set to `callers`, `callees`, or
`references`. Returned targets and source anchors can be passed to `vvv_navigate`
or `vvv_context` to inspect the declarations.
