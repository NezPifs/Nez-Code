# Configuration

Nez-Code reads `~/.nez/config.toml` (or `$NEZ_HOME/config.toml`), then
`.nez/config.toml` in trusted projects, then `-c key=value` flags. Later
sources win.

```toml
model_provider = "ollama"
model = "qwen3.5:9b"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
```

See [providers](providers.md), [local models](local-models.md) and
[planning and permissions](planning-and-permissions.md) for the settings
you are most likely to change.

## Profiles

A profile is a second config file layered on top of the main one:
`~/.nez/work.config.toml` is used with `nez --profile work`. Use profiles
to switch between, for example, a local model and DeepSeek without editing
`config.toml`.

## Feature flags

Optional and experimental behaviour sits behind feature flags.

```sh
nez features list              # every flag, its stage and whether it is on
nez --enable memories          # turn one on for a single run
```

or permanently:

```toml
[features]
memories = true
```

## MCP servers

Model Context Protocol servers add tools. Add one with `nez mcp add`:

```sh
nez mcp add docs --url https://example.com/mcp      # streamable HTTP
nez mcp add files -- npx -y @modelcontextprotocol/server-filesystem ~/notes
nez mcp list
```

Servers are stored under `[mcp_servers.<name>]` in `config.toml`. Their
tools go through the same approval prompt as built-in tools. Small local
models handle a few extra tools well and many poorly, so add only what you
need.

## Memories

With the `memories` feature on, Nez-Code keeps short notes about your
projects and preferences in `~/.nez/memories` and uses them in later
sessions. The notes are plain files you can read and edit.

```toml
[features]
memories = true

[memories]
generate_memories = true   # write new notes from finished sessions
use_memories = true        # include notes in new sessions
```

## Skills and instructions

- `AGENTS.md` in a project (and `~/.nez/AGENTS.md`) is added to the
  instructions for every session in that project.
- Skills live in `~/.nez/skills/<name>/SKILL.md` or `.nez/skills/` in a
  project; the model loads one when a task matches its description.

## Environment variables

| Variable | Effect |
| --- | --- |
| `NEZ_HOME` | Where config, sessions and logs live (default `~/.nez`). |
| `OLLAMA_HOST`, `NEZ_OSS_BASE_URL` | Ollama server to use. |
| `DEEPSEEK_API_KEY` | DeepSeek key; overrides a stored key. |
| `NEZ_VISION` | `auto`, `on` or `off`: whether images are sent. |
| `NEZ_ENABLE_THINKING` | `1` lets thinking models think on local turns. |
| `NEZ_DISABLE_NATIVE_TOOLS` | `1` uses the text tool protocol for every model. |
| `NEZ_FEEDBACK_DSN` | Sentry DSN for `/feedback` uploads; unset, feedback points to the issue tracker. |

## Telemetry

Nez-Code sends no telemetry by default. OpenTelemetry logs, traces and
metrics are only exported when you configure an exporter under `[otel]`,
for example to a local collector:

```toml
[otel]
exporter = { otlp-http = { endpoint = "http://localhost:4318/v1/logs", protocol = "binary" } }
```
