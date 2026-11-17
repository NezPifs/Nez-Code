"""Stand-in for the `nez` binary used by the dashboard tests.

Records its arguments to $FAKE_NEZ_LOG and replays a scripted
`nez exec --json` event stream.
"""
import json
import os
import sys
import time

args = sys.argv[1:]
log = os.environ.get("FAKE_NEZ_LOG")
if log:
    with open(log, "a", encoding="utf-8") as fh:
        fh.write(json.dumps(args) + "\n")

if args[:1] == ["--version"]:
    print("nez-cli 3.0.0")
    sys.exit(0)

if args[:1] == ["login"]:
    key = sys.stdin.read().strip()
    sys.exit(0 if key else 1)

if args[:1] == ["exec"]:
    if os.environ.get("FAKE_NEZ_SLEEP"):
        time.sleep(float(os.environ["FAKE_NEZ_SLEEP"]))
    resumed = len(args) > 2 and args[1] == "resume"
    thread = args[2] if resumed else "11111111-2222-3333-4444-555555555555"
    events = [
        {"type": "thread.started", "thread_id": thread},
        {"type": "turn.started"},
        {"type": "item.started", "item": {"id": "c1", "type": "command_execution",
                                           "command": "ls", "aggregated_output": "",
                                           "exit_code": None, "status": "in_progress"}},
        {"type": "item.completed", "item": {"id": "c1", "type": "command_execution",
                                             "command": "ls", "aggregated_output": "a.txt",
                                             "exit_code": 0, "status": "completed"}},
        {"type": "item.completed", "item": {"id": "m1", "type": "agent_message",
                                             "text": "résumé ok" if resumed else "first answer"}},
        {"type": "turn.completed", "usage": {"input_tokens": 1200, "cached_input_tokens": 0,
                                              "output_tokens": 34, "reasoning_output_tokens": 0}},
    ]
    for event in events:
        # Split writes to exercise partial-line buffering in the dashboard.
        line = (json.dumps(event, ensure_ascii=False) + "\n").encode("utf-8")
        sys.stdout.buffer.write(line[:7])
        sys.stdout.buffer.flush()
        sys.stdout.buffer.write(line[7:])
        sys.stdout.buffer.flush()
    sys.exit(0)

sys.exit(2)
