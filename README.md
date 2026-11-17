# Nez-Code

[![CI](https://github.com/NezPifs/Nez-Code/actions/workflows/ci.yml/badge.svg)](https://github.com/NezPifs/Nez-Code/actions/workflows/ci.yml)
[![CLI sandbox](https://github.com/NezPifs/Nez-Code/actions/workflows/cli-sandbox.yml/badge.svg)](https://github.com/NezPifs/Nez-Code/actions/workflows/cli-sandbox.yml)
[![Release](https://img.shields.io/github/v/release/NezPifs/Nez-Code?sort=semver)](https://github.com/NezPifs/Nez-Code/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Nez-Code is a local-first coding agent for your terminal. It reads your
project, runs commands, edits files and checks its own work, using models
that run on your machine through [Ollama](https://ollama.com). Nothing leaves
your computer unless you choose a hosted provider such as DeepSeek.

- **Local by default.** Picks a model that fits your GPU, sizes the context
  window from the model, and keeps small models on task with tuned sampling,
  a lean prompt, empty-reply recovery and loop guards.
- **Works with any model.** Native tool calls where the model supports them,
  and a JSON/XML text protocol for models that do not.
- **Safe by design.** Commands run in a sandbox, risky actions ask first, and
  allow/deny rules let you decide exactly what may run.
- **Vision.** Attach screenshots or diagrams; they reach models that can
  read images.
- **Desktop dashboard.** An optional PyQt app for chat with image paste,
  session history, model management and updates.

## Install

**macOS and Linux**

```sh
curl -fsSL https://github.com/NezPifs/Nez-Code/releases/latest/download/install.sh | sh
```

**Windows (PowerShell)**

```powershell
irm https://github.com/NezPifs/Nez-Code/releases/latest/download/install.ps1 | iex
```

The installer puts `nez` in `~/.nez/bin`, adds it to your `PATH`, and offers
to install Ollama and download a model sized for your GPU. Set
`NEZ_SKIP_OLLAMA=1` to skip that part, or `NEZ_NON_INTERACTIVE=1` to accept
the defaults. While the repository is private, export a `GITHUB_TOKEN` that
can read it first.

**From source** (Rust 1.95 or newer):

```sh
git clone https://github.com/NezPifs/Nez-Code.git
cd Nez-Code/nez-rs
cargo build --release --bin nez --bin nez-code-mode-host
./target/release/nez
```

## Quick start

```sh
cd my-project
nez                                 # interactive session
nez "add a --verbose flag"          # start with a prompt
nez exec "run the tests and fix any failures"   # non-interactive, prints the result
nez exec --json "summarize this repo"           # machine-readable events
```

The first interactive start asks where requests should go: local Ollama,
the DeepSeek API, or skip. Run `nez setup` to change it later.

Inside a session:

| Key or command | Does |
| --- | --- |
| `/model` | Switch to another installed model |
| `/permissions` | Change what Nez may do without asking |
| Shift+Tab | Toggle plan mode (think first, change nothing) |
| `/resume` | Continue an earlier session |
| Esc | Interrupt the current turn |

## Models

| GPU memory | Suggested model |
| --- | --- |
| 12 GB or more | `deepseek-coder-v2:16b` |
| 8 GB or more | `qwen3.5:9b` (default) |
| 6 GB | `qwen2.5-coder:7b-instruct` |

```sh
ollama pull deepseek-coder-v2:16b
nez --model deepseek-coder-v2:16b
```

To use DeepSeek instead:

```sh
printenv DEEPSEEK_API_KEY | nez login --provider deepseek --with-api-key
nez -c model_provider='"deepseek"' -m deepseek-chat
```

Keys are stored in the system credential store, never in config files.

## Configuration

Settings live in `~/.nez/config.toml`:

```toml
model_provider = "ollama"
model = "qwen3.5:9b"
approval_policy = "on-request"     # untrusted | on-request | never
sandbox_mode = "workspace-write"   # read-only | workspace-write | danger-full-access
```

## Updating

```sh
nez update            # download and install the latest release
nez update --check    # only report whether one is available
nez update --rollback # go back to the version the last update replaced
```

### Upgrading from 2.x

3.0.0 is a new codebase, so a 2.x install cannot update itself to it. Run
the installer above once; after that `nez update` works as usual. Settings
from `~/.nezcode` are not imported: run `nez setup` to pick a provider again.
See the [changelog](CHANGELOG.md) for everything that changed.

## Documentation

- [Local models](docs/local-models.md): tiers, context size, vision, thinking, loop guards
- [Providers](docs/providers.md): Ollama, DeepSeek, OpenAI-compatible servers, API keys
- [Tool calling](docs/tool-calling.md): native calls and the text protocol
- [Planning and permissions](docs/planning-and-permissions.md): plan mode, task list, sandbox, command rules
- [Configuration](docs/configuration.md): profiles, features, MCP, memories, environment variables
- [Testing](docs/testing.md): running the test suites and the CLI sandbox
- [Dashboard](dashboard/README.md)

## Privacy

Nez-Code sends no telemetry and no analytics by default. Requests go only to
the model provider you configure.

## Contributing

Bug reports and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md)
and the [code of conduct](CODE_OF_CONDUCT.md). Report security issues as
described in [SECURITY.md](SECURITY.md).

## License

Nez-Code is licensed under the [Apache License 2.0](LICENSE). It is based on
and modified from [openai/codex](https://github.com/openai/codex); see
[NOTICE](NOTICE).
