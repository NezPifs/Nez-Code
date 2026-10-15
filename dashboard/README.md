# Nez-Code dashboard

A desktop companion for the `nez` CLI, written with PyQt5. Every chat message
runs `nez exec --json` as a child process and follow-ups resume the same
thread, so the dashboard and the terminal share sessions and settings.

```sh
pip install -r dashboard/requirements.txt
python dashboard/nez_dashboard.py
```

The dashboard finds `nez` on `PATH`; set `NEZ_BIN` to use another binary.

## Tabs

- **Dashboard**: Ollama status, the active model and its vision support,
  the installed version, and update checks. New releases are detected
  automatically (this can be turned off) but never installed without a click
  on **Apply update** (`nez update --yes`). **Revert last update** goes back
  to the previous version (`nez update --rollback`).
- **Chat**: multi-line prompts (Ctrl+Enter sends),
  image attachments from a file or the clipboard (Ctrl+V) with thumbnails,
  Stop, elapsed time and token usage per turn, tool activity, and export of
  the transcript. New sessions start fresh; existing ones are resumed.
- **Models**: installed Ollama models with their capabilities (tools,
  vision, thinking, context length). **Use** writes the choice to
  `~/.nez/config.toml`.
- **Sessions**: browse and filter past sessions, preview them and resume one
  in the Chat tab.
- **Providers**: switch between Ollama and DeepSeek. API keys are passed to
  `nez login --provider ... --with-api-key` and kept in the system credential
  store.
- **Logs**: the latest `nez` log.
- **Backups**: archive a project folder (skipping `.git`, `node_modules` and
  build output) and restore it.

Nothing is captured from the camera or screen; images are only sent when you
attach them.

## Tests

The tests drive real `QProcess` children against `testdata/fake_nez.py`, a
scripted stand-in for the CLI:

```sh
QT_QPA_PLATFORM=offscreen python -m unittest discover -s dashboard
```
