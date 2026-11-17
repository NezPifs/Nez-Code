# Changelog

All notable changes to Nez-Code are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [3.0.0] - Unreleased

A major release. Nez-Code is rebuilt on a new open-source engine, a Rust
agent based on [openai/codex](https://github.com/openai/codex) (Apache-2.0),
adapted to run local models first. The 2.x behaviours that made small models
usable (model profiles, the text tool protocol, vision, loop guards) are
reimplemented on top of it.

### Added

- **Local models by default.** Ollama is the default provider with
  `qwen3.5:9b` as the default model. Setup and the installers suggest a
  model from detected GPU memory: 6 GB `qwen2.5-coder:7b-instruct`, 8 GB+
  `qwen3.5:9b`, 12 GB+ `deepseek-coder-v2:16b`.
- **Chat and Ollama transports.** `wire_api = "chat"` for any
  OpenAI-compatible server and `wire_api = "ollama"` for Ollama's native API,
  which sends `num_ctx`, `think` and JSON-schema `format`. The context window
  is sized from the model profile and capped at what `/api/show` reports.
- **DeepSeek provider** (`deepseek-chat`, `deepseek-reasoner`).
- **Model profiles.** Per-family sampling (temperature 0.2, top_p 0.9 for
  agent turns), context size and tool-calling mode.
- **Text tool protocol** for models without reliable native tool calls: JSON
  blocks, `{"message", "tool_calls"}` envelopes, `<shell>`, `<powershell>`,
  `<read_file>`, `<write_file>` and `<edit_file>` blocks, the 2.x tool names
  and DeepSeek's raw tool tokens all become normal, approved tool calls.
  Ollama replies are constrained with a JSON schema. `NEZ_DISABLE_NATIVE_TOOLS=1`
  forces the protocol.
- **Vision.** Attached images, pasted images and images returned by tools
  reach models that report vision support. A new image sent to a text-only
  model fails with a clear error. `NEZ_VISION=auto|on|off`.
- **Loop guards for local models.** One retry for empty replies, a stop after
  three identical failing calls, a goal reminder after six tool calls,
  collapsing of repeated tool output, and refusal of commands and patches that
  touch Nez-Code's own session state.
- **First-run setup** and `nez setup`: local Ollama (with model download),
  the DeepSeek API, or skip. `nez login --provider ID --with-api-key` and
  `nez logout --provider ID` manage keys, which are kept in the system
  credential store.
- **Model picker for local servers.** `/model` lists the models installed
  on Ollama (or returned by `/models`), with their size.
- **Task list on by default**, alongside plan mode (Shift+Tab, `/plan`) and
  allow/prompt/forbidden command rules in `~/.nez/rules/`.
- **`nez update`** with `--check` and `--rollback`, installing releases from
  GitHub after verifying their checksums.
- **Installers** (`install.sh`, `install.ps1`) that install the latest
  release and optionally Ollama and a model sized for the machine.
- **Release builds** for Linux (x64, arm64), macOS (arm64, x64) and Windows
  (x64), published as `nez-code-<version>-<platform>` with `SHA256SUMS`.
- **Dashboard.** A PyQt5 desktop companion that drives `nez exec --json`:
  chat with image attach and paste, session browsing and resume, a model
  panel with vision status, providers, logs, backups, and update checks with
  manual apply and revert.
- **End-to-end CLI tests.** A sandbox that runs the real binary against
  scripted model servers and checks files, command output and requests.

### Changed

- Configuration moved to `~/.nez/config.toml` (TOML). Sessions are stored
  under `~/.nez/sessions`.
- Local and third-party models get a short, action-oriented system prompt,
  and sub-agent and goal tools are left out of Ollama requests.
- Thinking is turned off for local agent turns; `NEZ_ENABLE_THINKING=1`
  turns it back on.
- Shell commands carry a `description` that is shown in approval prompts.
  PowerShell is the shell on Windows.
- The context meter scales its baseline to small windows.

### Removed

- The 2.x TypeScript codebase and its git-based `nez update`.
- Sign-in with a hosted account, device-code login, cloud tasks and the
  responses API proxy.
- Telemetry and analytics: no metrics, events or feedback are sent anywhere
  unless you configure an endpoint (`[otel]`, `NEZ_FEEDBACK_DSN`).
- Commit attribution instructions; the agent does not add co-author trailers
  or footers to commits and pull requests.
- npm, Homebrew and Bazel packaging.

### Upgrade notes

- **Reinstall once.** 3.0.0 does not share history with 2.x, so `nez update`
  in 2.x cannot reach it. Run the installer from the README; after that,
  `nez update` installs new releases.
- **Settings are not imported.** 2.x kept its settings in
  `~/.nezcode/config.json`. Run `nez setup` (or edit `~/.nez/config.toml`) to
  choose a provider and model again, and re-enter API keys with
  `nez login --provider deepseek --with-api-key`.
- **Sessions** from 2.x cannot be resumed in 3.0.0.
- **Dashboard.** Use the new `dashboard/nez_dashboard.py`; the 2.x Control
  Center does not work with the new CLI.

## 2.x history

2.x was a separate TypeScript codebase. Its last releases introduced the
behaviour that 3.0.0 rebuilds:

- **2.2.1** (2026-10-14): compound requests keep running after file edits;
  one recovery attempt for empty replies; first cloud CLI sandbox.
- **2.2.0** (2026-10-14): vision input with capability checks and
  `NEZ_VISION`; dashboard image attachments, export, session filtering and
  metrics.
- **2.1.169 to 2.1.177** (2026-06-06): native tool calling for local models;
  word-boundary profile matching for model sizes; lean local prompt; thinking
  disabled for local turns.
- **2.1.159 to 2.1.168** (2026-04-19 to 2026-04-20): file read, write, edit,
  search and list tools; context-aware auto-compaction for small windows;
  safer dashboard updates with revert; DeepSeek tool-token handling.
- **2.1.140 to 2.1.158** (2026-04-17 to 2026-04-19): the text tool bridge;
  per-profile sampling; internal-state protection; the repeat-failure breaker;
  the first-run provider wizard with DeepSeek; the first dashboard.
