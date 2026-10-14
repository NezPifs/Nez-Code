# Security Policy

## Supported versions

Only the latest release of Nez-Code receives security fixes. Run `nez update`
(or re-run the installer) before reporting an issue to confirm it still
reproduces.

| Version | Supported |
| ------- | --------- |
| 3.x     | Yes       |
| 2.x     | No        |

## Reporting a vulnerability

Please do not open a public issue for security problems.

Report vulnerabilities privately through
[GitHub security advisories](https://github.com/NezPifs/Nez-Code/security/advisories/new).
Include:

- the Nez-Code version (`nez --version`) and operating system,
- the model provider in use (local Ollama, DeepSeek, or another endpoint),
- steps to reproduce, and what an attacker gains.

You should get an acknowledgement within a week. Fixes are released as soon as
they are ready and credited in the changelog unless you ask otherwise.

## Scope

In scope:

- sandbox escapes and approval bypasses, including commands that run without
  the prompt the active approval policy requires,
- permission rules that fail open,
- API keys or other secrets written to disk in plain text or sent to a host
  other than the configured provider,
- the installers and `nez update` fetching or running unverified code.

Out of scope:

- behaviour of the language model itself. A model can be talked into
  proposing harmful commands; the approval prompt and sandbox are the
  security boundary, not the model.
- issues that need an attacker to already control your user account or your
  Ollama server.

## Operating Nez-Code safely

- Keep the default `workspace-write` sandbox and `on-request` approvals unless
  you understand what changes.
- Review the description and full command in every approval prompt. Nez-Code
  asks local models to describe mutating commands, but the command itself is
  what runs.
- Store hosted-provider keys with `nez login` or first-run setup; they go to
  the operating system keychain, not to `config.toml`.
