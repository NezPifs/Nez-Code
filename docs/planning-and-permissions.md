# Planning and permissions

## Plan mode

Plan mode is for working out an approach before anything changes. The model
reads and explores, asks questions, and proposes a plan; it does not edit
files or run commands that change state.

- Press **Shift+Tab** in the composer to cycle between the default mode and
  Plan mode. The footer shows `Plan mode` while it is active.
- `/plan` switches to Plan mode directly.
- When you are happy with the plan, switch back and ask the model to carry it
  out.

## Task list

For multi-step work the model keeps a short task list with the `update_plan`
tool: each step is `pending`, `in_progress` or `completed`, and the list is
shown in the conversation as it changes. It is on by default. To hide it:

```toml
[tools.update_plan]
enabled = false
```

## Approvals and sandbox

Two settings decide what the model may do without asking:

```toml
# untrusted | on-request | never
approval_policy = "on-request"
# read-only | workspace-write | danger-full-access
sandbox_mode = "workspace-write"
```

- `workspace-write` lets commands write inside the project (and temp
  directories) but nowhere else; network access is off unless enabled.
- `on-request` lets the model ask for approval when a command needs more than
  the sandbox allows. `untrusted` asks for anything not known to be safe.
  `never` returns failures to the model instead of asking.
- `/permissions` changes both for the current session.

### Linux sandbox

On Linux the sandbox uses [bubblewrap](https://github.com/containers/bubblewrap).
Install it with your package manager (`apt install bubblewrap`,
`dnf install bubblewrap`). Some distributions restrict unprivileged user
namespaces; on Ubuntu 24.04 and later run
`sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0` or add an
AppArmor profile for `bwrap`.

### Windows sandbox

On Windows, commands run under a restricted token with write access limited
to the project. The first time it is needed Nez-Code asks to set it up; the
elevated setup creates a dedicated local user so commands cannot reach your
other files. Without it, choose read-only mode or approve commands
individually.

Shell commands that change something carry a short description, shown in the
approval prompt. Commands and patches that touch Nez-Code's own session
files under `NEZ_HOME` are always refused.

## Command rules

Fine-grained allow and deny rules live in `~/.nez/rules/*.rules` (and
`.nez/rules/` in a trusted project). Each rule matches a command prefix:

```starlark
# Always allow read-only git commands.
prefix_rule(pattern = ["git", ["status", "diff", "log"]], decision = "allow")

# Ask before pushing.
prefix_rule(pattern = ["git", "push"], decision = "prompt")

# Never run these; the justification is shown to the model.
prefix_rule(
    pattern = ["rm", "-rf"],
    decision = "forbidden",
    justification = "Delete specific paths instead of using rm -rf.",
)
```

`decision` is `allow`, `prompt` or `forbidden`; the strictest matching rule
wins. Choosing "always allow" in an approval prompt adds an `allow` rule to
`~/.nez/rules/default.rules`.

Check what a rule file decides for a command:

```sh
nez execpolicy check --rules ~/.nez/rules/default.rules git push origin main
```
