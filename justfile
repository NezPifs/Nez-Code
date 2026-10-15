set working-directory := "nez-rs"
set positional-arguments

python := if os_family() == "windows" { "python" } else { "python3" }

# List recipes
help:
    just -l

# Run `nez` from source
nez *args:
    cargo run --bin nez -- "$@"

# Run `nez exec` from source
exec *args:
    cargo run --bin nez -- exec "$@"

# Format Rust code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Lint the whole workspace the same way CI does
clippy *args:
    cargo clippy --workspace --all-targets "$@" -- -D warnings

# Run tests; pass crate selectors such as `-p nez-api` to narrow the run
test *args:
    RUST_MIN_STACK=8388608 cargo test "$@"

# Run the tests CI runs
test-ci:
    ../scripts/ci-test.sh all

# Run the CLI tool sandbox against a debug build
sandbox:
    cargo build --bin nez
    {{ python }} ../scripts/cli-tool-sandbox.py --nez target/debug/nez

# Run the dashboard tests
[no-cd]
dashboard-test:
    QT_QPA_PLATFORM=offscreen {{ python }} -m unittest discover -s dashboard -p "test_*.py" -v

# Regenerate the json schema for config.toml
write-config-schema:
    cargo run -p nez-config-schema --bin nez-write-config-schema
