#!/usr/bin/env bash
# Runs the test suites CI gates on. Invoke from nez-rs/ with `crates` or `unit`.
#
# The full upstream integration suite is large and exercises hosted-service
# behaviour Nez-Code does not ship. CI runs the crates Nez-Code owns or
# depends on directly; run `cargo test -p <crate>` locally for anything else.
set -euo pipefail

suite="${1:-all}"
shift || true

crates=(
  nez-api
  nez-cli
  nez-config
  nez-exec
  nez-login
  nez-model-provider-info
  nez-model-profiles
  nez-models-manager
  nez-ollama
  nez-protocol
  nez-utils-oss
)

if [[ "$suite" == "crates" || "$suite" == "all" ]]; then
  args=()
  for crate in "${crates[@]}"; do
    args+=(-p "$crate")
  done
  cargo test --locked --no-fail-fast "${args[@]}" "$@"
fi

if [[ "$suite" == "unit" || "$suite" == "all" ]]; then
  # Unit tests only for the engine and terminal UI; their integration suites
  # cover hosted-service flows Nez-Code does not ship. Code mode tests spawn
  # the standalone host binary, so build it first.
  cargo build --locked -p nez-code-mode-host
  cargo test --locked --no-fail-fast -p nez-core -p nez-tui --lib "$@"
fi
