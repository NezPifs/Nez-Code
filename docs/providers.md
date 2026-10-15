# Providers

A provider is where model requests go. Nez-Code ships three:

| ID | Where | Protocol | Key |
| --- | --- | --- | --- |
| `ollama` (default) | your machine | Ollama native `/api/chat` | none |
| `deepseek` | `https://api.deepseek.com/v1` | chat completions | `DEEPSEEK_API_KEY` or keychain |
| `lmstudio` | `http://localhost:1234/v1` | chat completions | none |

## First-run setup

The first time you start `nez` in a terminal without a provider in
`~/.nez/config.toml`, Nez-Code asks where requests should go:

1. **Local models with Ollama.** Shows the detected GPU memory, recommends a
   model for it and offers to download it.
2. **DeepSeek API.** Asks for an API key without echoing it and stores it in
   the system credential store. If `DEEPSEEK_API_KEY` is already set, that is
   used instead.
3. **Skip.** Keeps the defaults (local Ollama) and does not ask again.

Run `nez setup` to choose again later. Setup never runs for `nez exec` or
when a provider is given with `-c model_provider=...`.

## Choosing a provider by hand

Pick one in `~/.nez/config.toml`:

```toml
model_provider = "deepseek"
model = "deepseek-chat"
```

or for a single run: `nez -c model_provider='"deepseek"' -m deepseek-chat`.

## API keys

Keys are never written to `config.toml`. First-run setup and
`nez login` store them in the operating system's credential store (Windows
Credential Manager, macOS Keychain, or the Secret Service on Linux):

```sh
printenv DEEPSEEK_API_KEY | nez login --provider deepseek --with-api-key
nez login status        # shows where the active provider's key comes from
nez logout --provider deepseek
```

An environment variable named by the provider's `env_key` (for DeepSeek,
`DEEPSEEK_API_KEY`) takes precedence over the stored key.

## Other OpenAI-compatible servers

Any server that implements `/v1/chat/completions` works:

```toml
model_provider = "vllm"
model = "Qwen/Qwen3-14B"

[model_providers.vllm]
name = "vLLM on the GPU box"
base_url = "http://gpu-box:8000/v1"
wire_api = "chat"
# env_key = "VLLM_API_KEY"   # if the server needs a bearer token
```

`wire_api` is one of:

- `chat`: OpenAI-compatible chat completions with SSE streaming,
- `ollama`: Ollama's native API (supports `num_ctx`, `think` and JSON-schema
  `format`),
- `responses`: the Responses API, for servers that implement it.

Tool calls, images and the text-protocol fallback work the same on every wire
API; see [tool calling](tool-calling.md).
