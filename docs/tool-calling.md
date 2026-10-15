# Tool calling

Nez-Code gives the model the same tools regardless of provider: a shell
(`exec_command`), file edits (`apply_patch`), the task list (`update_plan`),
image viewing and any MCP tools you configure. Every call goes through the
same approval prompt and sandbox, however the model asked for it.

## Native tool calls

Models whose profile supports it (see [local models](local-models.md)) receive
tools as native function definitions: Ollama's `tools` field, or the
`tools`/`tool_choice` fields of chat completions for hosted providers.
Calls stream back as structured `tool_calls` and are executed directly.

## Text protocol fallback

Small local models often handle native tool templates badly or not at all.
For those, Nez-Code leaves `tools` out of the request and describes a text
protocol in the system prompt instead. The model writes tool blocks in its
reply, Nez-Code turns them into ordinary tool calls, and the results come back
in the next message.

JSON blocks name a tool and its arguments:

```json
{"tool": "exec_command", "input": {"cmd": "cargo test"}}
```

Arrays of calls, fenced code blocks and the `{"name": ..., "arguments": ...}`
shape some models copy from function-calling docs are accepted too. Tool
names are matched case-insensitively. The names Nez 2.x taught still work:
`Bash` and `PowerShell` map to the shell tool, `ReadFile` to a read-only
command, and `WriteFile` and `EditFile` (with `path`, `content`,
`old_string`, `new_string`) to patches.

XML shorthands cover the common file and shell actions:

```xml
<powershell description="Create the build folder">New-Item -ItemType Directory build</powershell>
<read_file path="src/main.rs" start="1" end="80"/>
<write_file path="notes.txt">Full file contents</write_file>
<edit_file path="calc.py"><old>    return a - b</old><new>    return a + b</new></edit_file>
```

`<shell>`, `<bash>` and `<powershell>` run through `exec_command`.
`read_file` becomes a read-only command (`sed -n` or `Get-Content`).
`write_file` and `edit_file` become `apply_patch` calls, so file changes are
reviewed and applied exactly like native edits. DeepSeek Coder's raw
tool-call tokens are recognized as well.

A call to a tool that does not exist is still dispatched, so the model gets a
normal "unsupported call" result and can correct itself instead of stalling.
A reply whose only tool blocks cannot be run (a missing argument, or a file
edit when edits are unavailable) is sent back with the reason, and the model
gets two more tries before the turn ends.

### Structured output

When the server supports it, text-protocol requests also carry a JSON schema
(Ollama's `format`) that constrains the reply to:

```json
{"message": "text for the user", "tool_calls": [{"tool": "...", "input": {}}]}
```

This removes most malformed calls from small models. It is turned off for
requests that include images. When the caller asks for a structured final
answer (`nez exec --output-schema`), that schema is described in the prompt
rather than enforced, so the model can still call tools on the way there.

### Forcing the text protocol

If a model's native tool calls misbehave, switch it to the text protocol:

```sh
NEZ_DISABLE_NATIVE_TOOLS=1 nez
```

## Verifying changes

`scripts/cli-tool-sandbox.py` runs the real CLI against scripted model
responses for both native and text-protocol models and checks the files and
command output it produces. See [testing](testing.md).
