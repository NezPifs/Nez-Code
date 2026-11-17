#!/usr/bin/env python3
"""Runs the real `nez` CLI against scripted model responses.

Each case starts a throwaway model server that speaks Ollama's native API
(or OpenAI-compatible chat completions), points a fresh NEZ_HOME at it and
runs `nez exec --json` in a temporary project. Assertions look at real files,
real command output and the history sent back to the model; a success message
from the model alone never counts.

    python3 scripts/cli-tool-sandbox.py --nez nez-rs/target/debug/nez
    python3 scripts/cli-tool-sandbox.py --nez ... --case native-fix-and-verify
    python3 scripts/cli-tool-sandbox.py --nez ... --live   # proxy to a real Ollama

Results and raw request/response logs are written to $RUNNER_TEMP (or a temp
directory) under nez-sandbox-results/.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Callable, Optional

MODEL = "qwen3.5:9b"
SOURCE = "def add(a, b):\n    return a - b\n"
TEST = (
    "from calculator import add\n"
    "assert add(2, 3) == 5, 'addition is broken'\n"
    "assert add(-2, 3) == 1\n"
    "print('TESTS_PASSED')\n"
)
NOTE = "Fixed add() to use + instead of -.\n"
PROMPT = (
    "Read calculator.py and test_calculator.py, run the tests to reproduce the bug, "
    "fix calculator.py, rerun the tests, and create notes.txt explaining the fix."
)
FIX_PATCH = (
    "*** Begin Patch\n*** Update File: calculator.py\n@@\n"
    "-    return a - b\n+    return a + b\n*** End Patch\n"
)
NOTE_PATCH = "*** Begin Patch\n*** Add File: notes.txt\n+" + NOTE.rstrip("\n") + "\n*** End Patch\n"
DONE = "SANDBOX_DONE: the tests pass and notes.txt is written."


def python_cmd() -> str:
    return "python" if os.name == "nt" else "python3"


def shell(cmd: str) -> dict[str, Any]:
    return {"name": "exec_command", "arguments": {"cmd": cmd}}


def patch(text: str) -> dict[str, Any]:
    return {"name": "apply_patch", "arguments": {"input": text}}


@dataclass
class Turn:
    """One scripted model reply: text and/or tool calls."""

    text: str = ""
    calls: list[dict[str, Any]] = field(default_factory=list)
    thinking: str = ""


@dataclass
class Case:
    name: str
    turns: list[Turn]
    check: Callable[["Run"], list[str]]
    dialect: str = "ollama"  # or "openai"
    model: str = MODEL
    capabilities: list[str] = field(default_factory=lambda: ["completion", "tools"])
    prompt: str = PROMPT
    env: dict[str, str] = field(default_factory=dict)
    images: list[str] = field(default_factory=list)
    expect_exit: int = 0


@dataclass
class Run:
    case: Case
    project: Path
    exit_code: int
    events: list[dict[str, Any]]
    requests: list[dict[str, Any]]
    stdout: str
    stderr: str

    def items(self, kind: str) -> list[dict[str, Any]]:
        return [
            e["item"]
            for e in self.events
            if e.get("type") == "item.completed" and e.get("item", {}).get("type") == kind
        ]

    def commands(self) -> list[dict[str, Any]]:
        return self.items("command_execution")

    def history_text(self) -> str:
        return json.dumps(self.requests)

    def final_message(self) -> str:
        messages = self.items("agent_message")
        return messages[-1]["text"] if messages else ""


class ScriptedServer:
    def __init__(self, case: Case, live_upstream: Optional[str]) -> None:
        self.case = case
        self.live = live_upstream
        self.requests: list[dict[str, Any]] = []
        self.responses: list[Any] = []
        self.errors: list[str] = []
        self.turn = 0
        server = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args: Any) -> None:
                pass

            def _json(self, body: Any, status: int = 200) -> None:
                data = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def do_GET(self) -> None:  # noqa: N802
                if self.path.startswith("/api/tags"):
                    self._json({"models": [{"name": server.case.model}]})
                elif self.path.startswith("/api/version"):
                    self._json({"version": "0.14.0"})
                else:
                    self._json({"error": "not found"}, 404)

            def do_POST(self) -> None:  # noqa: N802
                length = int(self.headers.get("Content-Length", "0"))
                body = json.loads(self.rfile.read(length) or b"{}")
                try:
                    if self.path.startswith("/api/show"):
                        self._json({
                            "capabilities": server.case.capabilities,
                            "model_info": {"general.architecture": "qwen35", "qwen35.context_length": 262144},
                        })
                    elif self.path.startswith("/api/chat") or self.path.endswith("/chat/completions"):
                        server.requests.append(body)
                        if server.live:
                            server.proxy(self, body)
                        else:
                            server.reply(self, body)
                    else:
                        self._json({"error": f"unexpected path {self.path}"}, 404)
                except Exception as exc:  # noqa: BLE001 - recorded for the report
                    server.errors.append(repr(exc))
                    self._json({"error": str(exc)}, 500)

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    @property
    def port(self) -> int:
        return self.httpd.server_address[1]

    def __enter__(self) -> "ScriptedServer":
        self.thread.start()
        return self

    def __exit__(self, *_exc: Any) -> None:
        self.httpd.shutdown()

    def next_turn(self) -> Turn:
        turns = self.case.turns
        turn = turns[self.turn] if self.turn < len(turns) else Turn(text=DONE)
        self.turn += 1
        return turn

    def reply(self, handler: BaseHTTPRequestHandler, _body: dict[str, Any]) -> None:
        turn = self.next_turn()
        if self.case.dialect == "ollama":
            self._reply_ollama(handler, turn)
        else:
            self._reply_openai(handler, turn)

    def _reply_ollama(self, handler: BaseHTTPRequestHandler, turn: Turn) -> None:
        chunks: list[dict[str, Any]] = []
        if turn.thinking:
            chunks.append({"message": {"role": "assistant", "content": "", "thinking": turn.thinking}, "done": False})
        # Stream text in pieces to exercise incremental decoding.
        for piece in [turn.text[i:i + 12] for i in range(0, len(turn.text), 12)]:
            chunks.append({"message": {"role": "assistant", "content": piece}, "done": False})
        if turn.calls:
            chunks.append({"message": {"role": "assistant", "content": "", "tool_calls": [
                {"function": {"name": c["name"], "arguments": c["arguments"]}} for c in turn.calls
            ]}, "done": False})
        chunks.append({"message": {"role": "assistant", "content": ""}, "done": True,
                       "done_reason": "stop", "prompt_eval_count": 900, "eval_count": 40})
        self.responses.append(chunks)
        handler.send_response(200)
        handler.send_header("Content-Type", "application/x-ndjson")
        handler.end_headers()
        for chunk in chunks:
            handler.wfile.write((json.dumps(chunk) + "\n").encode())
            handler.wfile.flush()

    def _reply_openai(self, handler: BaseHTTPRequestHandler, turn: Turn) -> None:
        chunks: list[dict[str, Any]] = []
        if turn.text:
            chunks.append({"choices": [{"index": 0, "delta": {"role": "assistant", "content": turn.text}}]})
        for i, call in enumerate(turn.calls):
            args = json.dumps(call["arguments"])
            chunks.append({"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": i, "id": f"call_{self.turn}_{i}", "type": "function",
                 "function": {"name": call["name"], "arguments": args[: len(args) // 2]}}]}}]})
            chunks.append({"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": i, "function": {"arguments": args[len(args) // 2:]}}]}}]})
        chunks.append({"choices": [{"index": 0, "delta": {},
                                    "finish_reason": "tool_calls" if turn.calls else "stop"}]})
        chunks.append({"choices": [], "usage": {"prompt_tokens": 900, "completion_tokens": 40, "total_tokens": 940}})
        self.responses.append(chunks)
        handler.send_response(200)
        handler.send_header("Content-Type", "text/event-stream")
        handler.end_headers()
        for chunk in chunks:
            handler.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
            handler.wfile.flush()
        handler.wfile.write(b"data: [DONE]\n\n")

    def proxy(self, handler: BaseHTTPRequestHandler, body: dict[str, Any]) -> None:
        req = urllib.request.Request(
            f"{self.live}/api/chat", data=json.dumps(body).encode(),
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=600) as response:
            handler.send_response(200)
            handler.send_header("Content-Type", "application/x-ndjson")
            handler.end_headers()
            raw = b""
            for line in response:
                raw += line
                handler.wfile.write(line)
                handler.wfile.flush()
        self.responses.append(raw.decode(errors="replace"))


def write_config(home: Path, case: Case, port: int) -> None:
    lines = [
        f'model = "{case.model}"',
        'approval_policy = "never"',
        'sandbox_mode = "workspace-write"',
        "check_for_update_on_startup = false",
    ]
    if case.dialect == "openai":
        lines = [
            'model_provider = "sandbox"',
            *lines,
            "",
            "[model_providers.sandbox]",
            'name = "Sandbox"',
            f'base_url = "http://127.0.0.1:{port}/v1"',
            'wire_api = "chat"',
        ]
    else:
        lines.insert(0, 'model_provider = "ollama"')
    (home / "config.toml").write_text("\n".join(lines) + "\n", encoding="utf-8")


def run_case(nez: Path, case: Case, out: Path, live: Optional[str], timeout: int) -> dict[str, Any]:
    case_dir = out / case.name
    shutil.rmtree(case_dir, ignore_errors=True)
    project = case_dir / "project"
    home = case_dir / "home"
    project.mkdir(parents=True)
    home.mkdir()
    (project / "calculator.py").write_text(SOURCE, encoding="utf-8")
    (project / "test_calculator.py").write_text(TEST, encoding="utf-8")
    for image in case.images:
        shutil.copy(Path(__file__).resolve().parent / "sandbox-fixtures" / image, project / image)

    started = time.monotonic()
    with ScriptedServer(case, live) as server:
        write_config(home, case, server.port)
        env = {k: v for k, v in os.environ.items() if not k.startswith(("NEZ_", "OPENAI_", "DEEPSEEK_"))}
        env.update({
            "NEZ_HOME": str(home),
            "NEZ_OSS_BASE_URL": f"http://127.0.0.1:{server.port}/v1",
            "NO_COLOR": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
            **case.env,
        })
        args = [str(nez), "exec", "--json", "--skip-git-repo-check"]
        for image in case.images:
            args += ["--image", image]
        args += ["--", case.prompt]
        try:
            completed = subprocess.run(args, cwd=project, env=env, capture_output=True, text=True,
                                       stdin=subprocess.DEVNULL,
                                       timeout=timeout, encoding="utf-8", errors="replace")
            code, stdout, stderr = completed.returncode, completed.stdout, completed.stderr
        except subprocess.TimeoutExpired as exc:
            code = -1
            stdout = exc.stdout.decode(errors="replace") if isinstance(exc.stdout, bytes) else (exc.stdout or "")
            stderr = (exc.stderr.decode(errors="replace") if isinstance(exc.stderr, bytes) else (exc.stderr or "")) + "\n[timed out]"
        requests, responses, server_errors = server.requests, server.responses, server.errors

    events = []
    for line in stdout.splitlines():
        try:
            events.append(json.loads(line))
        except ValueError:
            pass
    run = Run(case, project, code, events, requests, stdout, stderr)
    failures = [] if code == case.expect_exit else [f"exit code {code}, expected {case.expect_exit}"]
    failures += case.check(run)
    failures += [f"server error: {e}" for e in server_errors]

    (case_dir / "stdout.jsonl").write_text(stdout, encoding="utf-8")
    (case_dir / "stderr.txt").write_text(stderr, encoding="utf-8")
    (case_dir / "requests.json").write_text(json.dumps(requests, indent=2), encoding="utf-8")
    (case_dir / "responses.json").write_text(json.dumps(responses, indent=2), encoding="utf-8")
    result = {
        "case": case.name,
        "passed": not failures,
        "failures": failures,
        "exit_code": code,
        "seconds": round(time.monotonic() - started, 1),
        "model_requests": len(requests),
        "commands": [c.get("command") for c in run.commands()],
        "final_message": run.final_message(),
    }
    (case_dir / "result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    return result


# --------------------------------------------------------------------------
# Checks


def fixed_and_verified(run: Run) -> list[str]:
    failures = []
    calc = (run.project / "calculator.py").read_text()
    if "return a + b" not in calc:
        failures.append("calculator.py was not fixed")
    notes = run.project / "notes.txt"
    if not notes.exists() or notes.read_text() != NOTE:
        failures.append("notes.txt missing or wrong")
    verify = subprocess.run([python_cmd(), "-B", "test_calculator.py"], cwd=run.project,
                            capture_output=True, text=True)
    if verify.returncode != 0:
        failures.append("fixture tests fail after the run")
    history = run.history_text()
    if "addition is broken" not in history:
        failures.append("the failing test output never reached the model")
    if "TESTS_PASSED" not in history:
        failures.append("the passing test output never reached the model")
    if "SANDBOX_DONE" not in run.final_message():
        failures.append("missing final answer")
    return failures


def fix_and_verify_turns() -> list[Turn]:
    py = python_cmd()
    return [
        Turn(text="Reading the files.", calls=[shell("cat calculator.py"), shell("cat test_calculator.py")]),
        Turn(calls=[shell(f"{py} test_calculator.py")]),
        Turn(text="The bug is a minus sign.", calls=[patch(FIX_PATCH)]),
        Turn(calls=[patch(NOTE_PATCH)]),
        Turn(calls=[shell(f"{py} test_calculator.py")]),
    ]


def envelope(message: str, *calls: dict[str, Any]) -> Turn:
    """A structured-output reply as a JSON-protocol model would send it."""
    return Turn(text=json.dumps({
        "message": message,
        "tool_calls": [{"tool": c["name"], "input": c["arguments"]} for c in calls],
    }))


def json_bridge_turns() -> list[Turn]:
    py = python_cmd()
    return [
        envelope("Reading the files.", shell("cat calculator.py"), shell("cat test_calculator.py")),
        envelope("", shell(f"{py} test_calculator.py")),
        envelope("The bug is a minus sign.", patch(FIX_PATCH)),
        envelope("", patch(NOTE_PATCH)),
        envelope("", shell(f"{py} test_calculator.py")),
        envelope(DONE),
    ]


def xml_bridge_turns() -> list[Turn]:
    py = python_cmd()
    return [
        Turn(text="Let me look.\n<read_file path=\"calculator.py\"/>\n<read_file path=\"test_calculator.py\"/>"),
        Turn(text=f"<shell description=\"Run the tests\">{py} test_calculator.py</shell>"),
        Turn(text="<edit_file path=\"calculator.py\"><old>    return a - b</old><new>    return a + b</new></edit_file>"),
        Turn(text=f"<write_file path=\"notes.txt\">{NOTE}</write_file>"),
        Turn(text=f"```json\n{{\"tool\": \"exec_command\", \"input\": {{\"cmd\": \"{py} test_calculator.py\"}}}}\n```"),
    ]


def text_protocol_used(run: Run) -> list[str]:
    if not run.requests:
        return ["no model request"]
    first = run.requests[0]
    failures = []
    if first.get("tools"):
        failures.append("native tools were sent to a text-protocol model")
    system = first["messages"][0]["content"]
    if "You cannot call tools natively" not in system or "exec_command" not in system:
        failures.append("the system prompt does not teach the text protocol")
    return failures


def structured_output_requested(run: Run) -> list[str]:
    fmt = run.requests[0].get("format") if run.requests else None
    if not isinstance(fmt, dict) or "tool_calls" not in json.dumps(fmt):
        return [f"expected a JSON-schema format, got {fmt!r}"]
    return []


def unknown_tool_reported(run: Run) -> list[str]:
    if len(run.requests) < 2:
        return ["the error never reached the model"]
    if "DeleteEverything" not in json.dumps(run.requests[1]["messages"]):
        return ["the unknown-tool error was not sent back"]
    return []


def single_read(run: Run) -> list[str]:
    failures = []
    if len(run.requests) < 2:
        return ["the tool result was never sent back"]
    second = json.dumps(run.requests[1]["messages"])
    if "return a - b" not in second:
        failures.append("file contents did not reach the model")
    return failures


def native_tools_sent(run: Run) -> list[str]:
    if not run.requests:
        return ["no model request"]
    first = run.requests[0]
    failures = []
    names = [t["function"]["name"] for t in first.get("tools", [])]
    if "exec_command" not in names:
        failures.append(f"exec_command missing from native tools: {names}")
    if run.case.dialect == "ollama":
        opts = first.get("options", {})
        if opts.get("num_ctx") != 16384:
            failures.append(f"num_ctx should be 16384, got {opts.get('num_ctx')}")
        if opts.get("temperature") != 0.2 or opts.get("top_p") != 0.9:
            failures.append(f"agent sampling not applied: {opts}")
    return failures


def empty_reply_recovered(run: Run) -> list[str]:
    failures = []
    if len(run.requests) < 2:
        return ["the empty reply was not retried"]
    retry = run.requests[1]
    if retry.get("tools"):
        failures.append("the retry still sent native tools")
    last = retry["messages"][-1]
    if last.get("role") != "user" or "Your last reply was empty" not in str(last.get("content")):
        failures.append(f"the retry did not nudge the model: {last!r}")
    if not any("cat calculator.py" in c.get("command", "") for c in run.commands()):
        failures.append("the tool call from the retry did not run")
    return failures


def repeated_failure_stopped(run: Run) -> list[str]:
    failures = []
    if len(run.requests) != 3:
        failures.append(f"expected 3 model requests before stopping, got {len(run.requests)}")
    if "failed 3 times in a row" not in run.final_message():
        failures.append(f"final message does not explain the stop: {run.final_message()!r}")
    return failures


def internal_state_refused(run: Run) -> list[str]:
    if len(run.requests) < 2:
        return ["the refusal never reached the model"]
    history = json.dumps(run.requests[1]["messages"])
    failures = []
    if "Refusing to touch" not in history:
        failures.append("the command was not refused")
    if run.commands():
        failures.append("a command ran against NEZ_HOME")
    return failures


def image_sent(run: Run) -> list[str]:
    if not run.requests:
        return ["no model request"]
    user = [m for m in run.requests[0]["messages"] if m.get("role") == "user"]
    if not any(m.get("images") for m in user):
        return ["the attached image did not reach the vision model"]
    return []


def text_only_image_rejected(run: Run) -> list[str]:
    failures = []
    if run.requests:
        failures.append("the request was sent to a text-only model")
    if "cannot read images" not in run.stdout + run.stderr:
        failures.append("no clear error for the text-only model")
    return failures


def combine(*checks: Callable[[Run], list[str]]) -> Callable[[Run], list[str]]:
    return lambda run: [f for check in checks for f in check(run)]


def cases() -> list[Case]:
    return [
        Case("native-fix-and-verify", fix_and_verify_turns(), combine(fixed_and_verified, native_tools_sent)),
        Case("openai-dialect-fix-and-verify", fix_and_verify_turns(),
             combine(fixed_and_verified, native_tools_sent), dialect="openai", model="deepseek-chat"),
        Case("single-read", [Turn(calls=[shell("cat calculator.py")])], single_read,
             prompt="Show me calculator.py."),
        Case("json-bridge-fix-and-verify", json_bridge_turns(),
             combine(fixed_and_verified, text_protocol_used, structured_output_requested),
             model="deepseek-coder-v2:16b", capabilities=["completion"]),
        Case("xml-bridge-fix-and-verify", xml_bridge_turns(),
             combine(fixed_and_verified, text_protocol_used),
             env={"NEZ_DISABLE_NATIVE_TOOLS": "1"}),
        Case("text-unknown-tool", [envelope("", {"name": "DeleteEverything", "arguments": {}})],
             unknown_tool_reported, model="deepseek-coder-v2:16b", capabilities=["completion"],
             prompt="Clean up."),
        Case("empty-reply-recovery", [Turn(), envelope("", shell("cat calculator.py"))],
             empty_reply_recovered, prompt="Show me calculator.py."),
        Case("repeated-failure-stops",
             [Turn(calls=[shell(f"{python_cmd()} missing_script.py")]) for _ in range(4)],
             repeated_failure_stopped, prompt="Run missing_script.py."),
        Case("internal-state-refused", [Turn(calls=[shell("ls $NEZ_HOME/sessions")])],
             internal_state_refused, prompt="List my past Nez sessions."),
        Case("vision-image-attached", [Turn(text="It is a red square.")], image_sent,
             capabilities=["completion", "tools", "vision"], images=["red-square.png"],
             prompt="What is in this image?"),
        Case("text-only-model-rejects-image", [], text_only_image_rejected,
             images=["red-square.png"], prompt="What is in this image?", expect_exit=1),
    ]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--nez", required=True, type=Path, help="path to the nez binary")
    parser.add_argument("--case", action="append", help="run only these cases")
    parser.add_argument("--live", nargs="?", const="http://127.0.0.1:11434",
                        help="forward model requests to a real Ollama server")
    parser.add_argument("--timeout", type=int, default=180)
    opts = parser.parse_args()

    nez = opts.nez.resolve()
    if not nez.exists():
        print(f"nez binary not found: {nez}", file=sys.stderr)
        return 2
    out = Path(os.environ.get("RUNNER_TEMP") or tempfile.gettempdir()) / "nez-sandbox-results"
    out.mkdir(parents=True, exist_ok=True)

    selected = [c for c in cases() if not opts.case or c.name in opts.case]
    if opts.live:
        selected = [c for c in selected if c.dialect == "ollama"][:1]
        for case in selected:
            case.check = fixed_and_verified
    results = []
    for case in selected:
        result = run_case(nez, case, out, opts.live, opts.timeout * (4 if opts.live else 1))
        results.append(result)
        status = "PASS" if result["passed"] else "FAIL"
        print(f"{status} {case.name} ({result['seconds']}s, {result['model_requests']} model requests)", flush=True)
        for failure in result["failures"]:
            print(f"    - {failure}", flush=True)
        if not result["passed"]:
            print(f"    logs: {out / case.name}", flush=True)

    (out / "summary.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as fh:
            fh.write("| Case | Result | Model requests | Seconds |\n|---|---|---:|---:|\n")
            for r in results:
                fh.write(f"| {r['case']} | {'pass' if r['passed'] else 'FAIL'} | {r['model_requests']} | {r['seconds']} |\n")
    return 0 if all(r["passed"] for r in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
