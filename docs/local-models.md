# Local models

Nez-Code runs against [Ollama](https://ollama.com) by default. Nothing leaves
your machine unless you configure a hosted provider.

## Default model and hardware tiers

The built-in default is `qwen3.5:9b`. First-run setup (and the installers)
look at your GPU memory and suggest a tier:

| GPU memory | Model | Notes |
| --- | --- | --- |
| 12 GB or more | `deepseek-coder-v2:16b` | Dense 16B code model; uses the JSON tool protocol. |
| 8 GB or more | `qwen3.5:9b` | Default. Native tool calls, 16k context. |
| 6 GB | `qwen2.5-coder:7b-instruct` | Fits in about 4.5 GB; uses the JSON tool protocol. |

GPU memory is read from `nvidia-smi`, the `amdgpu` driver on Linux, the
display adapter registry entry on Windows, or unified memory on Apple
Silicon. When nothing can be detected the default model is used.

Nez-Code only downloads the built-in default on its own. For anything else,
pull it yourself and select it:

```sh
ollama pull deepseek-coder-v2:16b
nez --model deepseek-coder-v2:16b
```

or set it permanently in `~/.nez/config.toml`:

```toml
model_provider = "ollama"
model = "deepseek-coder-v2:16b"
```

Mixture-of-experts models such as `qwen3.x:35b-a3b` are not recommended for
agent work on a 16 GB card: only a few billion parameters are active per
token, while the full weights still overflow VRAM and spill to the CPU.

## Context size

Ollama truncates prompts at its own small default context unless the client
asks for more. Nez-Code sends `num_ctx` on every request:

- the model profile's context size (16k for `qwen3.5:9b`, 32k for larger
  Qwen models, 8k for small JSON-protocol models), or `model_context_window`
  from `config.toml` when set,
- capped at the context length the model reports through `/api/show`.

The same value drives auto-compaction, so long sessions are summarized before
the model starts losing the beginning of the conversation.

```toml
# Ask for a larger window on a card with headroom.
model_context_window = 32768
```

## Model profiles

Each model family has a profile that sets sampling and tool calling:

| Profile | Models | Tool calls | Context |
| --- | --- | --- | --- |
| `qwen-9b` | `qwen3.5:9b` | native | 16k |
| `qwen-large` | Qwen 13B and larger, MoE builds, untagged community builds | native | 32k |
| `qwen-small` | Qwen 8B and smaller | JSON text protocol | 8k |
| `deepseek-coder` | `deepseek-coder-v2:*` | JSON text protocol | 16k |
| `gpt-oss` | `gpt-oss:*` | native | 32k |
| `json-tools` | Llama, Mistral, Gemma, Phi, Hermes, CodeLlama, StarCoder, Granite | native when Ollama reports tool support, otherwise JSON | 8k |
| `deepseek-api` | hosted `deepseek-chat`, `deepseek-reasoner` | native | 128k |

Agent turns use `temperature 0.2` and `top_p 0.9` for every profile. Parameter
sizes are matched as whole tokens, so `qwen3:27b` is never mistaken for a 7B
model.

See [tool calling](tool-calling.md) for how the text protocol works.

## Picking a model

`/model` lists the models installed on the Ollama server (from `/api/tags`),
with their size and quantization. Embedding models are left out. When the
server cannot be reached, the built-in default is offered.

## Prompt and tools

Local models get a short, action-oriented system prompt instead of the full
one written for hosted frontier models: read before editing, verify after
changing, keep going until the task is done, prefer acting over asking.
Sub-agent and goal tools are left out of local requests; their definitions
alone would take a fifth of a 16k context.

## Images

Images you attach (`--image`, paste, or drag and drop) and images returned by
tools are sent to models that can read them. Nez-Code asks Ollama's
`/api/show` whether the model has the `vision` capability and caches the
answer for five minutes.

- If the model is text-only and your new message has an image, the turn stops
  with an error naming the model, so you can switch to a vision model such as
  `qwen2.5vl:7b`.
- Images from earlier in the session are replaced with a short note for
  text-only models, so one screenshot does not break the rest of the session.
- Servers that report no capabilities get the images.

`NEZ_VISION` overrides the check: `on` always sends images, `off` never does,
`auto` (the default) follows the server.

## Thinking

Thinking models such as Qwen 3 can spend their whole output budget reasoning
and never call a tool. Nez-Code sends `think: false` on local agent turns.
Set `NEZ_ENABLE_THINKING=1` to let the model think.

## Staying on task

A few checks keep small models from stalling or looping:

- **Empty replies.** If the model returns nothing (or only thinking), the
  request is retried once using the JSON tool protocol and an explicit
  reminder. If that is empty too, the turn ends with a note saying the task
  may be unfinished.
- **Repeated failures.** When the same tool call fails three times in a row,
  the turn stops and shows the last error instead of retrying forever.
- **Goal reminder.** After six tool calls without a new message from you, the
  request ends with a reminder of what you asked for.
- **Duplicate output.** When the same call returns the same long output more
  than once, earlier copies are shortened to save context.
- **Internal state.** Shell commands and patches that touch Nez-Code's own
  session files, credentials or databases under `NEZ_HOME` are refused.
  `config.toml`, `AGENTS.md`, skills and rules stay editable.

Every shell command that changes something should carry a short
`description`; it is shown in the approval prompt. On Windows the shell is
PowerShell.

## Pointing at another Ollama server

```sh
OLLAMA_HOST=gpu-box:11434 nez
```

`NEZ_OSS_BASE_URL` takes precedence if you need a full URL, for example
`http://gpu-box:11434/v1`.

## Troubleshooting

- **"No running Ollama server detected"**: start it with `ollama serve`, or
  install it from <https://ollama.com/download>. In the interactive UI this is
  a warning so you can still switch providers.
- **"Model `x` is not installed"**: run `ollama pull x` or pick an installed
  model with `/model`.
- **Ollama too old**: Nez-Code needs Ollama 0.9 or newer for native thinking
  control and streamed tool calls.
