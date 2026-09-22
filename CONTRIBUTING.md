# Contributing to vvv

vvv is a Rust workspace: a library with thin interfaces around it. Issues and pull
requests are welcome.

## Setup

Rust **1.90** or newer (`rust-version` in `Cargo.toml`), with `rustfmt` and `clippy`.
Nothing else: the language grammars are compiled in behind Cargo features.

## Build and test

```console
cargo build                                             # default features: rust, typescript
cargo test --workspace --all-features                   # includes the corpus gate
cargo clippy --workspace --all-targets --all-features
cargo fmt --all
dprint check                                             # markdown, JSON and TOML
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p vvv-rs --no-default-features             # the CLI with no languages
```

CI (`.github/workflows/ci.yml`) runs these with `RUSTFLAGS=-D warnings` on Linux, macOS
and Windows, plus a feature matrix and `dprint` (Linux). A pull request keeps them green.
`dprint fmt` writes the markdown, JSON and TOML back; the full local checklist, matrix
included, is in [AGENTS.md](AGENTS.md).

## Layout

[docs/architecture.md](docs/architecture.md) is the authority on the crates and the rules
that keep them apart; [AGENTS.md](AGENTS.md) is the short form. In one line: behaviour
lives in `vvv-engine`, the interfaces (`crates/vvv`, `vvv-tui`) talk only to it, and a
language is a module in `vvv-lang` behind a feature.

## What a change touches

- **Human output** is snapshotted with `insta`. Review the diff, then accept it:
  `INSTA_UPDATE=always cargo test -p vvv-rs` (CLI) or `-p vvv-tui` (picker).
- **What a command means** (a verdict, an address, an edit) is covered by the corpus
  gate: `crates/vvv/tests/corpus.rs` runs every command over the workspaces under
  `crates/vvv/tests/corpus/`. Read the snapshot diff as the review of the change, then
  accept it with `INSTA_UPDATE=always cargo test -p vvv-rs --test corpus`. A new shape of
  code vvv should handle goes into the corpus first, as a case that shows it.
- **A flag, a key, or what a command rewrites** → `docs/guide.md`.
- **JSON output** → `docs/protocol.md`.
- **A new language** → a feature and a module in `vvv-lang`, never a crate.
- **A new engine feature** → testable with the `Fake` language in
  `crates/vvv-engine/tests/common/mod.rs`.

## Commits and pull requests

The `cliff.toml` changelog is generated from commit subjects, so each subject is one line
that says what changed. Keep a pull request to one change, and keep unrelated formatting
out of it. Run the commands above before opening it.

## Reporting

Open an issue at <https://github.com/roushou/vvv/issues> with the command you ran, its
output, and what you expected. vvv reads syntax, not types: an occurrence it cannot place
is a `?` row to show, not a resolver bug on its own.

## License

MIT. A contribution is licensed the same way.
