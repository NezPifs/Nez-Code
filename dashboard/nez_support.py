"""Qt-independent helpers for the Nez-Code dashboard.

Everything here is plain Python so it can be unit tested without a display:
building `nez exec` command lines, decoding the JSONL event stream, reading
sessions and config, backups, and release checks.
"""

from __future__ import annotations

import base64
import codecs
import json
import os
import re
import shutil
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable, Optional

# The CLI reads image files itself; this keeps uploads well under what local
# vision models handle comfortably.
MAX_IMAGE_BYTES = 3 * 1024 * 1024
MAX_IMAGES = 4
IMAGE_SUFFIX = {
    "image/png": ".png",
    "image/jpeg": ".jpg",
    "image/gif": ".gif",
    "image/webp": ".webp",
}

DEFAULT_LOCAL_MODEL = "qwen3.5:9b"
DEFAULT_OLLAMA_URL = "http://localhost:11434"
RELEASES_API = "https://api.github.com/repos/NezPifs/Nez-Code/releases/latest"
BACKUP_SKIP_DIRS = {".git", "node_modules", ".venv", "__pycache__", "dist", "target", "coverage"}


# --------------------------------------------------------------------------
# Paths


def nez_home() -> Path:
    """Config home, honouring NEZ_HOME like the CLI does."""
    override = os.environ.get("NEZ_HOME")
    if override:
        return Path(override).expanduser()
    return Path.home() / ".nez"


def config_path() -> Path:
    return nez_home() / "config.toml"


def sessions_dir() -> Path:
    return nez_home() / "sessions"


def log_dir() -> Path:
    return nez_home() / "log"


def backups_dir() -> Path:
    return nez_home() / "backups"


def dashboard_state_path() -> Path:
    return nez_home() / "dashboard.json"


# --------------------------------------------------------------------------
# Images


def sniff_image_mime(data: bytes) -> Optional[str]:
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        return "image/png"
    if data.startswith(b"\xff\xd8\xff"):
        return "image/jpeg"
    if data[:6] in (b"GIF87a", b"GIF89a"):
        return "image/gif"
    if data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "image/webp"
    return None


@dataclass
class ImageAttachment:
    name: str
    path: Path
    size: int
    mime: str
    temporary: bool = False

    def cleanup(self) -> None:
        if self.temporary:
            try:
                self.path.unlink()
            except OSError:
                pass


def validate_image(data: bytes) -> str:
    """Returns the MIME type or raises ValueError with a user-facing message."""
    if not data:
        raise ValueError("The image is empty.")
    if len(data) > MAX_IMAGE_BYTES:
        raise ValueError("Each image must be at most 3 MiB.")
    mime = sniff_image_mime(data)
    if mime is None:
        raise ValueError("Choose a PNG, JPEG, WebP or GIF image.")
    return mime


def attachment_from_file(path: Path) -> ImageAttachment:
    data = path.read_bytes()
    mime = validate_image(data)
    return ImageAttachment(name=path.name, path=path, size=len(data), mime=mime)


def attachment_from_bytes(data: bytes, name: str, directory: Optional[Path] = None) -> ImageAttachment:
    """Stores pasted image bytes in a temporary file the CLI can read."""
    mime = validate_image(data)
    handle, raw = tempfile.mkstemp(prefix="nez-paste-", suffix=IMAGE_SUFFIX[mime], dir=directory)
    with os.fdopen(handle, "wb") as fh:
        fh.write(data)
    return ImageAttachment(name=name, path=Path(raw), size=len(data), mime=mime, temporary=True)


def image_data_url(attachment: ImageAttachment) -> str:
    data = attachment.path.read_bytes()
    return f"data:{attachment.mime};base64," + base64.b64encode(data).decode("ascii")


# --------------------------------------------------------------------------
# Command lines


@dataclass
class ChatRequest:
    prompt: str
    images: list[ImageAttachment] = field(default_factory=list)
    thread_id: Optional[str] = None
    model: Optional[str] = None
    provider: Optional[str] = None
    full_auto: bool = True
    skip_git_check: bool = True


def build_exec_args(request: ChatRequest) -> list[str]:
    """Arguments for `nez` (without the program name).

    New conversations use `nez exec`; follow-ups resume the recorded thread
    so the model keeps the whole transcript.
    """
    if len(request.images) > MAX_IMAGES:
        raise ValueError(f"Attach at most {MAX_IMAGES} images per message.")
    prompt = request.prompt.strip() or ("Describe these images." if request.images else "")
    if not prompt:
        raise ValueError("Type a message first.")
    args = ["exec"]
    if request.thread_id:
        args += ["resume", request.thread_id]
    args.append("--json")
    if request.skip_git_check:
        args.append("--skip-git-repo-check")
    if request.full_auto:
        args.append("--full-auto")
    if request.model:
        args += ["--model", request.model]
    if request.provider:
        args += ["-c", f"model_provider={json.dumps(request.provider)}"]
    for image in request.images:
        args += ["--image", str(image.path)]
    args += ["--", prompt]
    return args


# --------------------------------------------------------------------------
# Event stream


@dataclass
class StreamOutput:
    kind: str  # "assistant", "tool", "reasoning", "error", "info"
    text: str


class ExecStream:
    """Decodes `nez exec --json` output across arbitrary read boundaries."""

    def __init__(self) -> None:
        self._decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self._pending = ""
        self.thread_id: Optional[str] = None
        self.usage: dict[str, int] = {}
        self.failed = False
        self.finished = False
        self.last_message = ""

    def feed(self, data: bytes, final: bool = False) -> list[StreamOutput]:
        self._pending += self._decoder.decode(data, final=final)
        lines = self._pending.split("\n")
        self._pending = lines.pop()
        if final and self._pending:
            lines.append(self._pending)
            self._pending = ""
        out: list[StreamOutput] = []
        for line in lines:
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
            except ValueError:
                out.append(StreamOutput("info", line))
                continue
            if isinstance(event, dict):
                out.extend(self._handle(event))
        return out

    def _handle(self, event: dict[str, Any]) -> list[StreamOutput]:
        kind = event.get("type")
        if kind == "thread.started":
            self.thread_id = event.get("thread_id") or self.thread_id
            return []
        if kind == "turn.completed":
            usage = event.get("usage") or {}
            self.usage = {k: int(v) for k, v in usage.items() if isinstance(v, (int, float))}
            self.finished = True
            return []
        if kind == "turn.failed":
            self.failed = True
            self.finished = True
            message = (event.get("error") or {}).get("message", "The turn failed.")
            return [StreamOutput("error", message)]
        if kind == "error":
            self.failed = True
            return [StreamOutput("error", event.get("message", "Request failed."))]
        if kind in ("item.started", "item.completed"):
            return self._item(event.get("item") or {}, completed=kind == "item.completed")
        return []

    def _item(self, item: dict[str, Any], completed: bool) -> list[StreamOutput]:
        item_type = item.get("type")
        if item_type == "agent_message" and completed:
            self.last_message = item.get("text", "")
            return [StreamOutput("assistant", self.last_message)]
        if item_type == "reasoning" and completed and item.get("text"):
            return [StreamOutput("reasoning", item["text"])]
        if item_type == "command_execution":
            command = item.get("command", "")
            if not completed:
                return [StreamOutput("tool", f"$ {command}")]
            status = item.get("status", "completed")
            code = item.get("exit_code")
            suffix = f" (exit {code})" if code is not None else ""
            if status == "declined":
                return [StreamOutput("tool", f"declined: {command}")]
            return [StreamOutput("tool", f"{status}{suffix}: {command}")]
        if item_type == "file_change" and completed:
            changes = ", ".join(
                f"{c.get('kind', 'update')} {c.get('path', '')}" for c in item.get("changes") or []
            )
            return [StreamOutput("tool", f"files {item.get('status', '')}: {changes}")]
        if item_type == "todo_list" and completed:
            lines = [
                ("[x] " if entry.get("completed") else "[ ] ") + entry.get("text", "")
                for entry in item.get("items") or []
            ]
            return [StreamOutput("tool", "plan:\n" + "\n".join(lines))] if lines else []
        if item_type == "error":
            return [StreamOutput("error", item.get("message", ""))]
        return []


def format_usage(usage: dict[str, int]) -> str:
    if not usage:
        return ""
    parts = [f"{usage.get('input_tokens', 0):,} in", f"{usage.get('output_tokens', 0):,} out"]
    cached = usage.get("cached_input_tokens", 0)
    if cached:
        parts.append(f"{cached:,} cached")
    return " / ".join(parts)


def format_elapsed(seconds: float) -> str:
    seconds = max(0, int(seconds))
    minutes, secs = divmod(seconds, 60)
    return f"{minutes}m {secs:02d}s" if minutes else f"{secs}s"


# --------------------------------------------------------------------------
# Sessions


@dataclass
class SessionInfo:
    path: Path
    thread_id: str
    started: datetime
    cwd: str
    title: str

    def label(self) -> str:
        return f"{self.started:%Y-%m-%d %H:%M}  {self.title}"


_ROLLOUT_RE = re.compile(
    r"rollout-(\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2})-([0-9a-fA-F-]{36})\.jsonl$"
)


def _first_user_text(lines: Iterable[str]) -> str:
    for raw in lines:
        try:
            obj = json.loads(raw)
        except ValueError:
            continue
        payload = obj.get("payload") if isinstance(obj, dict) else None
        if not isinstance(payload, dict):
            continue
        if payload.get("type") == "user_message" and payload.get("message"):
            return str(payload["message"])
        if payload.get("type") == "message" and payload.get("role") == "user":
            for part in payload.get("content") or []:
                text = part.get("text") if isinstance(part, dict) else None
                if text and not text.lstrip().startswith("<"):
                    return text
    return ""


def read_session(path: Path) -> Optional[SessionInfo]:
    match = _ROLLOUT_RE.search(path.name)
    if not match:
        return None
    try:
        started = datetime.strptime(match.group(1), "%Y-%m-%dT%H-%M-%S")
    except ValueError:
        return None
    cwd = ""
    try:
        with path.open("r", encoding="utf-8", errors="replace") as fh:
            head = [fh.readline() for _ in range(60)]
    except OSError:
        return None
    for raw in head[:1]:
        try:
            meta = json.loads(raw)
            cwd = (meta.get("payload") or {}).get("cwd", "")
        except (ValueError, AttributeError):
            pass
    first = _first_user_text(head).strip()
    title = first.splitlines()[0][:80] if first else ""
    return SessionInfo(
        path=path,
        thread_id=match.group(2),
        started=started,
        cwd=cwd,
        title=title or Path(cwd).name or "(empty session)",
    )


def list_sessions(root: Optional[Path] = None) -> list[SessionInfo]:
    root = root or sessions_dir()
    if not root.exists():
        return []
    sessions = [s for s in (read_session(p) for p in root.rglob("rollout-*.jsonl")) if s]
    sessions.sort(key=lambda s: s.started, reverse=True)
    return sessions


def filter_sessions(sessions: list[SessionInfo], query: str) -> list[SessionInfo]:
    query = query.strip().lower()
    if not query:
        return sessions
    return [s for s in sessions if query in s.label().lower() or query in s.cwd.lower()]


def session_preview(path: Path, limit: int = 200) -> str:
    """Readable transcript of a session file."""
    out: list[str] = []
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError as exc:
        return f"Could not read {path}: {exc}"
    for raw in lines:
        try:
            obj = json.loads(raw)
        except ValueError:
            continue
        payload = obj.get("payload") if isinstance(obj, dict) else None
        if not isinstance(payload, dict):
            continue
        kind = payload.get("type")
        if kind == "user_message":
            out.append("You: " + str(payload.get("message", "")))
        elif kind == "agent_message":
            out.append("Nez: " + str(payload.get("message", "")))
        elif kind == "function_call":
            out.append(f"[tool] {payload.get('name', '')} {payload.get('arguments', '')}"[:400])
        if len(out) >= limit:
            out.append("…")
            break
    return "\n\n".join(out) if out else "(no messages)"


# --------------------------------------------------------------------------
# Config


def read_top_level(text: str) -> dict[str, str]:
    """Top-level `key = value` pairs of a TOML file, values unquoted."""
    values: dict[str, str] = {}
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            break
        match = re.match(r'^([A-Za-z0-9_-]+)\s*=\s*(.+?)\s*(#.*)?$', stripped)
        if match:
            raw = match.group(2)
            if raw.startswith('"') and raw.endswith('"'):
                try:
                    raw = json.loads(raw)
                except ValueError:
                    raw = raw[1:-1]
            values[match.group(1)] = raw
    return values


def set_top_level(text: str, key: str, value: str) -> str:
    """Sets a top-level string key, preserving the rest of the file."""
    lines = text.splitlines()
    rendered = f"{key} = {json.dumps(value)}"
    first_table = next((i for i, l in enumerate(lines) if l.strip().startswith("[")), len(lines))
    pattern = re.compile(rf"^\s*{re.escape(key)}\s*=")
    for i in range(first_table):
        if pattern.match(lines[i]):
            lines[i] = rendered
            return "\n".join(lines) + "\n"
    insert_at = first_table
    while insert_at > 0 and not lines[insert_at - 1].strip():
        insert_at -= 1
    lines.insert(insert_at, rendered)
    return "\n".join(lines) + "\n"


def load_config() -> dict[str, str]:
    try:
        return read_top_level(config_path().read_text(encoding="utf-8"))
    except OSError:
        return {}


def save_model_selection(model: str, provider: str = "ollama") -> None:
    path = config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    text = path.read_text(encoding="utf-8") if path.exists() else ""
    text = set_top_level(text, "model_provider", provider)
    text = set_top_level(text, "model", model)
    tmp = path.with_suffix(".toml.tmp")
    tmp.write_text(text, encoding="utf-8")
    os.replace(tmp, path)


def current_model() -> tuple[str, str]:
    """(provider, model) from config, with the CLI's defaults."""
    cfg = load_config()
    provider = cfg.get("model_provider", "ollama")
    model = cfg.get("model") or (DEFAULT_LOCAL_MODEL if provider == "ollama" else "")
    return provider, model


def load_dashboard_state() -> dict[str, Any]:
    try:
        return json.loads(dashboard_state_path().read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def save_dashboard_state(state: dict[str, Any]) -> None:
    path = dashboard_state_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(state, indent=2), encoding="utf-8")


# --------------------------------------------------------------------------
# Ollama


def ollama_url() -> str:
    host = os.environ.get("OLLAMA_HOST", "").strip()
    if not host:
        return DEFAULT_OLLAMA_URL
    if not host.startswith("http"):
        host = "http://" + host
    return host.rstrip("/")


def _http_json(url: str, body: Optional[dict] = None, timeout: float = 4.0, headers: Optional[dict] = None) -> Any:
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json", **(headers or {})})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def ollama_models(base: Optional[str] = None) -> list[str]:
    data = _http_json(f"{base or ollama_url()}/api/tags")
    return sorted(m.get("name", "") for m in data.get("models", []) if m.get("name"))


def ollama_loaded(base: Optional[str] = None) -> list[dict[str, Any]]:
    data = _http_json(f"{base or ollama_url()}/api/ps")
    return list(data.get("models", []))


@dataclass
class ModelCapabilities:
    vision: Optional[bool]
    tools: Optional[bool]
    context_length: Optional[int]

    def vision_label(self) -> str:
        if self.vision is True:
            return "Vision: supported"
        if self.vision is False:
            return "Vision: not supported by this model (text only)"
        return "Vision: unknown (the server will validate images)"


def parse_show(data: dict[str, Any]) -> ModelCapabilities:
    caps = data.get("capabilities")
    vision = tools = None
    if isinstance(caps, list):
        vision = "vision" in caps
        tools = "tools" in caps
    context = None
    for key, value in (data.get("model_info") or {}).items():
        if key.endswith(".context_length") and isinstance(value, int):
            context = value
            break
    return ModelCapabilities(vision=vision, tools=tools, context_length=context)


def ollama_capabilities(model: str, base: Optional[str] = None) -> ModelCapabilities:
    return parse_show(_http_json(f"{base or ollama_url()}/api/show", {"model": model}))


# --------------------------------------------------------------------------
# Backups


def create_backup(project: Path, destination: Optional[Path] = None) -> Path:
    destination = destination or backups_dir()
    destination.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    archive = destination / f"{project.name or 'project'}-{stamp}.tar.gz"

    def skip(info: tarfile.TarInfo) -> Optional[tarfile.TarInfo]:
        parts = Path(info.name).parts
        return None if any(p in BACKUP_SKIP_DIRS for p in parts) else info

    with tarfile.open(archive, "w:gz") as tar:
        tar.add(project, arcname=project.name or "project", filter=skip)
    return archive


def list_backups(directory: Optional[Path] = None) -> list[Path]:
    directory = directory or backups_dir()
    if not directory.exists():
        return []
    return sorted(directory.glob("*.tar.gz"), key=lambda p: p.stat().st_mtime, reverse=True)


def safe_extract(archive: Path, destination: Path) -> None:
    """Extracts a backup, refusing members that escape `destination`."""
    destination = destination.resolve()
    with tarfile.open(archive, "r:gz") as tar:
        members = tar.getmembers()
        for member in members:
            target = (destination / member.name).resolve()
            if target != destination and destination not in target.parents:
                raise ValueError(f"Refusing to extract {member.name}: outside the destination.")
            if member.issym() or member.islnk():
                raise ValueError(f"Refusing to extract link {member.name}.")
        if hasattr(tarfile, "data_filter"):
            tar.extractall(destination, members=members, filter="data")
        else:
            tar.extractall(destination, members=members)


# --------------------------------------------------------------------------
# Logs


def latest_log(directory: Optional[Path] = None) -> Optional[Path]:
    directory = directory or log_dir()
    if not directory.exists():
        return None
    files = [p for p in directory.iterdir() if p.is_file()]
    return max(files, key=lambda p: p.stat().st_mtime) if files else None


def tail(path: Path, max_bytes: int = 200_000) -> str:
    with path.open("rb") as fh:
        fh.seek(0, os.SEEK_END)
        size = fh.tell()
        fh.seek(max(0, size - max_bytes))
        return fh.read().decode("utf-8", errors="replace")


# --------------------------------------------------------------------------
# Updates


def parse_version(text: str) -> Optional[tuple[int, int, int]]:
    match = re.search(r"(\d+)\.(\d+)\.(\d+)", text)
    return tuple(int(g) for g in match.groups()) if match else None  # type: ignore[return-value]


def is_newer(latest: str, current: str) -> bool:
    a, b = parse_version(latest), parse_version(current)
    return bool(a and b and a > b)


def github_token() -> Optional[str]:
    return os.environ.get("NEZ_GITHUB_TOKEN") or os.environ.get("GITHUB_TOKEN") or None


def latest_release(timeout: float = 6.0) -> dict[str, Any]:
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "nez-dashboard"}
    token = github_token()
    if token:
        headers["Authorization"] = f"Bearer {token}"
    try:
        return _http_json(RELEASES_API, timeout=timeout, headers=headers)
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            raise RuntimeError(
                "No release found. For the private repository set NEZ_GITHUB_TOKEN to a token with read access."
            ) from exc
        raise


def which(name: str) -> Optional[str]:
    return shutil.which(name)
