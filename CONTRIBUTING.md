# Contributing to Nez-Code

Thanks for helping out. Nez-Code is a private project for the owner and invited
friends, so contributions come from people with repository access.

## Getting set up

You need:

- Rust (the toolchain is pinned in `nez-rs/rust-toolchain.toml`; `rustup`
  installs it automatically),
- on Linux, `libcap-dev` and `pkg-config` for the bundled bubblewrap sandbox,
- Python 3.10+ and PyQt5 if you work on the dashboard,
- optionally [Ollama](https://ollama.com) to try changes against a real model.

```sh
git clone https://github.com/NezPifs/Nez-Code.git
cd Nez-Code/nez-rs
cargo build --bin nez
./target/debug/nez --help
```

The `justfile` at the repository root wraps the common commands
(`just fmt`, `just clippy`, `just test`).

## Workflow

1. Branch from `main` using a descriptive prefix: `feat/`, `fix/`, `docs/`,
   `ci/`, `refactor/` or `chore/`.
2. Keep commits small and focused. Commit messages follow
   [Conventional Commits](https://www.conventionalcommits.org/):

   ```text
   feat(tui): show the active model profile in the status line
   fix(ollama): read context length from model_info on older servers
   ```

   Mark breaking changes with `!` and explain them in the body.
3. Open a pull request into `main` and fill in the template. CI must be green
   before merging.

## Before you push

From `nez-rs/`:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <crate you changed>
```

Dashboard changes:

```sh
QT_QPA_PLATFORM=offscreen python -m unittest discover -s dashboard -p "test_*.py"
```

Changes to tool calling or the agent loop should also pass the CLI sandbox,
which runs the real binary against scripted model responses:

```sh
python3 scripts/cli-tool-sandbox.py --nez nez-rs/target/debug/nez
```

## Tests

Every behaviour change needs a test. Prefer unit tests next to the code
(`foo_tests.rs` files included with `#[path]`) and keep integration tests
under each crate's `tests/` directory. Snapshot tests use
[insta](https://insta.rs); review changes with `cargo insta review` instead of
accepting them blindly.

## Changelog

User-visible changes get an entry under **Unreleased** in `CHANGELOG.md`, in
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

## Licensing

Nez-Code is distributed under the Apache License 2.0 and is derived from
[openai/codex](https://github.com/openai/codex). Do not modify `LICENSE`, and
keep the attribution in `NOTICE`. By contributing you agree that your changes
are released under the same license.
