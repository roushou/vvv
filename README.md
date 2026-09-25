<p align="center">
  <img
    width="1792"
    height="1008"
    alt="vvv — veni, vidi, vici"
    src="https://github.com/user-attachments/assets/a7cd9b71-0e5c-49b0-8a67-44201cf82945"
  >
</p>

<h1 align="center">vvv</h1>

<p align="center">
  <a href="https://github.com/roushou/vvv/actions/workflows/ci.yml"><img src="https://github.com/roushou/vvv/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/vvv-rs"><img src="https://img.shields.io/crates/v/vvv-rs.svg" alt="crates.io"></a>
  <a href="https://docs.rs/vvv-rs"><img src="https://img.shields.io/docsrs/vvv-rs" alt="docs.rs"></a>
  <a href="https://github.com/roushou/vvv/blob/main/LICENSE"><img src="https://img.shields.io/crates/l/vvv-rs.svg" alt="license"></a>
</p>

<p align="center">
  <em>Rename, move and rewrite code by syntax, not string matching.</em>
</p>

vvv reads declarations, identifiers and imports, so a rename follows every reference and
a move rewrites every path that pointed at it. Nothing is written until you have seen the
diff, and every write can be undone.

## Install

```sh
cargo install vvv-rs
```

Prebuilt binaries for Linux (`x86_64`, `aarch64`), macOS (`x86_64`, `aarch64`) and
Windows are attached to each [release](https://github.com/roushou/vvv/releases).

> [!NOTE]
> The binary is `vvv`. The default build includes Rust, TypeScript and the
> picker. To install a subset, name the features, e.g.
> `--no-default-features --features rust,tui`.

## Commands

Every command runs against the current directory (`-C <dir>` picks another) and prints
human output (`--json` prints the machine-readable result).

### Find

```sh
vvv search <name>                       # every identifier spelling the name
vvv search --symbol <kind>              # declarations of one kind: function, struct, trait, …
vvv search '<pattern>'                  # ast-grep structural pattern, e.g. 'fn $N($$$) { $$$ }'
```

### Understand

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

### Change

```sh
vvv rename <name> <new>                 # the declaration and every reference
vvv move <from> <to>                    # a file or directory, and the paths pointing at it
vvv move --symbol <name> <from> <to>    # one declaration between files
vvv rewrite '<pattern>' '<template>'    # pattern matches, template expanded
vvv batch <file.json>                   # several intents as one transaction; - reads stdin
```

> [!NOTE]
> A changing command previews and stops. `--apply` writes the plan, `--diff`
> prints the full patch of every file, and `--select` acts on chosen rows only.

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

## Documentation

- [docs/guide.md](https://github.com/roushou/vvv/blob/main/docs/guide.md) — every command in detail, and the full mark legend.
- [docs/protocol.md](https://github.com/roushou/vvv/blob/main/docs/protocol.md) — the JSON contract.
- [docs/architecture.md](https://github.com/roushou/vvv/blob/main/docs/architecture.md) — the crates and their rules.
- [CHANGELOG.md](https://github.com/roushou/vvv/blob/main/CHANGELOG.md) — what changed in each release.
- [CONTRIBUTING.md](https://github.com/roushou/vvv/blob/main/CONTRIBUTING.md) — build, test and send a change.

## License

[MIT](./LICENSE)
