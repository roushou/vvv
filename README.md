# vvv

<img width="1792" height="1008" alt="grok-image-8b65c39d-e4c6-4538-b559-4142447f5b0e" src="https://github.com/user-attachments/assets/a7cd9b71-0e5c-49b0-8a67-44201cf82945" />

[![CI](https://github.com/roushou/vvv/actions/workflows/ci.yml/badge.svg)](https://github.com/roushou/vvv/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/vvv-rs.svg)](https://crates.io/crates/vvv-rs)
[![docs.rs](https://img.shields.io/docsrs/vvv-rs)](https://docs.rs/vvv-rs)
[![license](https://img.shields.io/crates/l/vvv-rs.svg)](https://github.com/roushou/vvv/blob/main/LICENSE)

vvv renames, moves and rewrites code across a project using syntax, not string
matching. It reads declarations, identifiers and imports, so a rename follows every
reference and a move rewrites every path that pointed at it. Nothing is written until
you have seen the diff, and every write can be undone.

## Install

```sh
cargo install vvv-rs
```

Prebuilt binaries for Linux (`x86_64`, `aarch64`), macOS (`x86_64`, `aarch64`) and
Windows are attached to each [release](https://github.com/roushou/vvv/releases).

The binary is `vvv`. The default build includes Rust, TypeScript and the picker. To
install a subset, name the features, e.g. `--no-default-features --features rust,tui`.

## Commands

Every command runs against the current directory (`-C <dir>` picks another) and prints
human output (`--json` prints the machine-readable result).

**Find**

```sh
vvv search <name>                       # every identifier spelling the name
vvv search --symbol <kind>              # declarations of one kind: function, struct, trait, …
vvv search '<pattern>'                  # ast-grep structural pattern, e.g. 'fn $N($$$) { $$$ }'
```

**Understand**

```sh
vvv outline <file>                      # declarations, visibility, module path
vvv references <name>                   # every use, judged like a rename
vvv where <name> --from <file>          # the declaration, and the import to write there
vvv deps <file>                         # what the file imports, and who imports it
vvv explain <file>:<line>:<column>      # what is at a position
vvv surface <package>                   # what a package offers, and who takes it
vvv impact <name>                       # who would feel a change, ring by ring
vvv dead                                # declarations nothing refers to
vvv imports <file>                      # unresolved, unused or redundant imports
```

**Change**

```sh
vvv rename <name> <new>                 # the declaration and every reference
vvv move <from> <to>                    # a file or directory, and the paths pointing at it
vvv move --symbol <name> <from> <to>    # one declaration between files
vvv rewrite '<pattern>' '<template>'    # pattern matches, template expanded
vvv batch <file.json>                   # several intents as one transaction; - reads stdin
```

A changing command previews and stops. `--apply` writes the plan, `--diff` prints the
full patch of every file, and `--select` acts on chosen rows only.

**After**

```sh
vvv undo                                # reverse the newest apply
vvv history                             # the ledger, oldest first
```

**Interfaces**

```sh
vvv                                     # the picker, in a terminal
vvv serve                               # JSON lines in, JSON lines out
```

## How it decides

vvv reads syntax, not types. It knows what a declaration is, what an identifier is, and
how modules and imports connect; it does not know which struct a method belongs to.

A changing command judges every candidate and prints it with a verdict:

- `✓` refers to the declaration you named;
- `?` cannot be tied to it — a bare name with no import, a method reached through a
  type, a path whose head cannot be placed;
- `✗` refers to a different declaration of the same name.

When several declarations share the name, `--in <file>` chooses one. `--select` narrows
the occurrences by the row numbers printed, or by the content ids from `--json`. Nothing
is dropped quietly.

A preview of a rename reads:

```sh
$ vvv rename Language Lang
rename Language → Lang

● trait Language   src/lang/mod.rs:64:1

✓ 3  3 files
  1  src/lang/mod.rs
  1  src/lib.rs
  1  src/other.rs

? 1  1 file   ∅ unresolved   left out
src/other.rs
  4     10:5    Language::new()

✗ 0
✓ 3  ? 1  ✗ 0   3 files   → 2 occurrences in 2 files
hint: --apply to write · --select 4 for the ? rows alone · -v to list the ✓ rows
```

The plan records the content of every file it touches, so it refuses to apply to a file
that changed since the preview. Apply is all-or-nothing, and `vvv undo` reverses the
newest apply.

## Output

Every command draws from one set of marks:

| mark        | meaning                                              |
| ----------- | ---------------------------------------------------- |
| `●`         | a declaration                                        |
| `○`         | a use, not a declaration                             |
| `→` `←`     | an import / imported by                              |
| `↗`         | a re-export                                          |
| `✓` `?` `✗` | refers to the declaration / can't tell / another one |
| `±`         | a structural edit: a `mod` line, a visibility        |
| `!`         | left to you, with the reason                         |
| `∅`         | nothing                                              |
| `+N`        | the match continues for N more lines                 |

Rows are numbered in the order printed; those numbers are what `--select` takes.

## Options

| flag                          | meaning                                                                                                   |
| ----------------------------- | --------------------------------------------------------------------------------------------------------- |
| `-C <dir>`                    | run against this project root                                                                             |
| `--json`                      | print the machine-readable result ([protocol](https://github.com/roushou/vvv/blob/main/docs/protocol.md)) |
| `--color auto\|always\|never` | colour policy; `NO_COLOR` is respected                                                                    |
| `-v`                          | expand what the human view collapses                                                                      |
| `--diff`                      | print a preview's full patch, not only structural edits                                                   |

## Limits

vvv follows functions, structs, enums, traits, type aliases, constants and modules
through imports, `crate::`/`self::`/`super::` paths, Cargo workspaces, `pub use` /
`export … from` re-exports, and relative TypeScript imports including `index` files.
What it cannot place is reported, not skipped.

Not handled: `tsconfig` path aliases, `#[path]` modules, code inside macro bodies, and
glob imports of a module whose symbol moves. A change vvv cannot make becomes a `!` row
with the text it would have written. Method and field renames match by name and are
narrowed by selection, because syntax alone does not say which type they belong to.

## JSON and sessions

`--json` prints one documented document per command, errors included, each with a stable
`code` and a `hint`. `vvv serve` reads the same commands as JSON lines on stdin and
answers on stdout, keeping the tree between requests. The contract is in
[docs/protocol.md](https://github.com/roushou/vvv/blob/main/docs/protocol.md).

## Documentation

- [docs/guide.md](https://github.com/roushou/vvv/blob/main/docs/guide.md) — every command in detail, and the full mark legend.
- [docs/protocol.md](https://github.com/roushou/vvv/blob/main/docs/protocol.md) — the JSON contract.
- [docs/architecture.md](https://github.com/roushou/vvv/blob/main/docs/architecture.md) — the crates and their rules.
- [CHANGELOG.md](https://github.com/roushou/vvv/blob/main/CHANGELOG.md) — what changed in each release.
- [CONTRIBUTING.md](https://github.com/roushou/vvv/blob/main/CONTRIBUTING.md) — build, test and send a change.

## License

[MIT](./LICENSE)
