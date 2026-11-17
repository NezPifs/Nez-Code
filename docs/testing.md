# Testing

## Unit and crate tests

From `nez-rs/`:

```sh
cargo test -p nez-api -p nez-model-profiles   # one crate at a time is fastest
../scripts/ci-test.sh crates                   # the crates CI gates on
../scripts/ci-test.sh unit                     # engine and TUI unit tests
```

The TUI and core suites are large. On machines with less than 16 GB of RAM,
limit parallelism with `CARGO_BUILD_JOBS=2` and run them with an 8 MiB test
stack (`RUST_MIN_STACK=8388608`), as CI does.

Snapshot tests use [insta](https://insta.rs). Review changes with
`cargo insta review` rather than editing `.snap` files by hand.

## CLI tool sandbox

`scripts/cli-tool-sandbox.py` runs the real `nez` binary end to end. A small
HTTP server plays the model: it answers with scripted replies (native tool
calls, JSON envelopes or XML blocks) and records every request. Each case runs
in a fresh workspace holding a broken `calculator.py` and its test. A case
passes only when the files on disk, the command output and the recorded
requests match what the script expects.

```sh
cargo build --bin nez
python3 scripts/cli-tool-sandbox.py --nez nez-rs/target/debug/nez
python3 scripts/cli-tool-sandbox.py --nez nez-rs/target/debug/nez --case xml-bridge-fix-and-verify
```

| Case | Checks |
| --- | --- |
| `native-fix-and-verify` | Native Ollama tool calls fix the bug, write a note and rerun the tests; `num_ctx` and agent sampling are sent. |
| `openai-dialect-fix-and-verify` | The same flow over chat completions (DeepSeek-style). |
| `single-read` | A read result reaches the model on the next request. |
| `json-bridge-fix-and-verify` | A JSON-protocol model: no native tools, protocol in the system prompt, JSON-schema `format`. |
| `xml-bridge-fix-and-verify` | XML shorthands with `NEZ_DISABLE_NATIVE_TOOLS=1`. |
| `text-unknown-tool` | A call to a tool that does not exist comes back to the model as an error. |

With `--live [URL]` the scripted server is replaced by a real Ollama server
and only the outcome is checked. CI runs that against a small CPU model when
the **CLI tool sandbox** workflow is started by hand with *live model*
enabled. Results for every run are uploaded as workflow artifacts.

## Installers

`scripts/test-install.sh` runs `install.sh` against a local stand-in for the
GitHub releases API and checks a fresh install, a reinstall, a checksum
mismatch and a refused daemon invocation. CI runs it on Linux and checks
that `install.ps1` parses on Windows.
