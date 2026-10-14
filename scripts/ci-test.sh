#!/usr/bin/env bash
# Runs the test suites CI gates on. Invoke from nez-rs/.
#
# The full upstream integration suite is large and exercises hosted-service
# behaviour Nez-Code does not ship. CI runs the crates Nez-Code owns or
# depends on directly; run `cargo test -p <crate>` locally for anything else.
set -euo pipefail

crates=(
  nez-api
  nez-cli
  nez-config
  nez-exec
  nez-login
  nez-model-provider-info
  nez-models-manager
  nez-ollama
  nez-protocol
  nez-utils-oss
)

args=()
for crate in "${crates[@]}"; do
  args+=(-p "$crate")
done

cargo test --locked --no-fail-fast "${args[@]}" "$@"
