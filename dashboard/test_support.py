"""Tests for the Qt-independent dashboard helpers."""

import io
import json
import os
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import nez_support as ns  # noqa: E402

# A valid 1x1 RGB PNG.
PNG = b'\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde\x00\x00\x00\x0cIDATx\x9cc\xf8\xcf\xc0\x00\x00\x03\x01\x01\x00\xc9\xfe\x92\xef\x00\x00\x00\x00IEND\xaeB`\x82'


class ImageTests(unittest.TestCase):
    def test_sniffs_supported_formats(self):
        self.assertEqual(ns.sniff_image_mime(PNG), "image/png")
        self.assertEqual(ns.sniff_image_mime(b"\xff\xd8\xff\xe0rest"), "image/jpeg")
        self.assertEqual(ns.sniff_image_mime(b"GIF89a....."), "image/gif")
        self.assertEqual(ns.sniff_image_mime(b"RIFF\x00\x00\x00\x00WEBPVP8 "), "image/webp")
        self.assertIsNone(ns.sniff_image_mime(b"%PDF-1.7"))

    def test_rejects_empty_large_and_unknown_files(self):
        with self.assertRaises(ValueError):
            ns.validate_image(b"")
        with self.assertRaises(ValueError):
            ns.validate_image(PNG + b"\x00" * ns.MAX_IMAGE_BYTES)
        with self.assertRaises(ValueError):
            ns.validate_image(b"not an image")

    def test_pasted_bytes_become_a_temporary_file_that_cleans_up(self):
        with tempfile.TemporaryDirectory() as tmp:
            attachment = ns.attachment_from_bytes(PNG, "pasted.png", Path(tmp))
            self.assertTrue(attachment.path.exists())
            self.assertEqual(attachment.path.suffix, ".png")
            self.assertTrue(ns.image_data_url(attachment).startswith("data:image/png;base64,"))
            attachment.cleanup()
            self.assertFalse(attachment.path.exists())


class CommandTests(unittest.TestCase):
    def test_new_conversation_uses_exec_json(self):
        args = ns.build_exec_args(ns.ChatRequest(prompt="fix the bug"))
        self.assertEqual(args[:2], ["exec", "--json"])
        self.assertIn("--full-auto", args)
        self.assertEqual(args[-2:], ["--", "fix the bug"])

    def test_follow_ups_resume_the_thread_and_pass_images(self):
        with tempfile.TemporaryDirectory() as tmp:
            image = Path(tmp) / "shot.png"
            image.write_bytes(PNG)
            attachment = ns.attachment_from_file(image)
            args = ns.build_exec_args(
                ns.ChatRequest(prompt="", images=[attachment], thread_id="abc", model="qwen3.5:9b")
            )
        self.assertEqual(args[:4], ["exec", "resume", "abc", "--json"])
        self.assertEqual(args[args.index("--image") + 1], str(image))
        self.assertEqual(args[args.index("--model") + 1], "qwen3.5:9b")
        self.assertEqual(args[-1], "Describe these images.")

    def test_rejects_empty_prompts_and_too_many_images(self):
        with self.assertRaises(ValueError):
            ns.build_exec_args(ns.ChatRequest(prompt="   "))
        fake = ns.ImageAttachment("a.png", Path("a.png"), 1, "image/png")
        with self.assertRaises(ValueError):
            ns.build_exec_args(ns.ChatRequest(prompt="x", images=[fake] * (ns.MAX_IMAGES + 1)))

    def test_provider_override_is_quoted(self):
        args = ns.build_exec_args(ns.ChatRequest(prompt="hi", provider="deepseek"))
        self.assertEqual(args[args.index("-c") + 1], 'model_provider="deepseek"')


class StreamTests(unittest.TestCase):
    def test_decodes_events_across_arbitrary_chunk_boundaries(self):
        events = [
            {"type": "thread.started", "thread_id": "t-1"},
            {"type": "item.started", "item": {"type": "command_execution", "command": "ls"}},
            {"type": "item.completed", "item": {"type": "command_execution", "command": "ls",
                                                 "exit_code": 0, "status": "completed"}},
            {"type": "item.completed", "item": {"type": "agent_message", "text": "héllo"}},
            {"type": "turn.completed", "usage": {"input_tokens": 10, "cached_input_tokens": 4,
                                                  "output_tokens": 2}},
        ]
        raw = "".join(json.dumps(e, ensure_ascii=False) + "\n" for e in events).encode("utf-8")
        stream = ns.ExecStream()
        outputs = []
        for i in range(0, len(raw), 5):
            outputs += stream.feed(raw[i:i + 5])
        outputs += stream.feed(b"", final=True)
        self.assertEqual(stream.thread_id, "t-1")
        self.assertTrue(stream.finished)
        self.assertFalse(stream.failed)
        self.assertEqual([o.kind for o in outputs], ["tool", "tool", "assistant"])
        self.assertEqual(outputs[-1].text, "héllo")
        self.assertEqual(ns.format_usage(stream.usage), "10 in / 2 out / 4 cached")

    def test_failures_and_plain_lines_are_reported(self):
        stream = ns.ExecStream()
        out = stream.feed(b'not json\n{"type":"turn.failed","error":{"message":"model not found"}}\n')
        self.assertEqual([(o.kind, o.text) for o in out], [("info", "not json"), ("error", "model not found")])
        self.assertTrue(stream.failed)

    def test_plan_items_render_as_a_checklist(self):
        stream = ns.ExecStream()
        event = {"type": "item.completed", "item": {"type": "todo_list", "items": [
            {"text": "read", "completed": True}, {"text": "fix", "completed": False}]}}
        out = stream.feed((json.dumps(event) + "\n").encode())
        self.assertEqual(out[0].text, "plan:\n[x] read\n[ ] fix")

    def test_elapsed_formatting(self):
        self.assertEqual(ns.format_elapsed(5.4), "5s")
        self.assertEqual(ns.format_elapsed(125), "2m 05s")


class SessionTests(unittest.TestCase):
    def write_session(self, root: Path, stamp: str, thread: str, message: str) -> Path:
        day = root / "2026" / "10" / "14"
        day.mkdir(parents=True, exist_ok=True)
        path = day / f"rollout-{stamp}-{thread}.jsonl"
        lines = [
            {"type": "session_meta", "payload": {"id": thread, "cwd": "/work/app"}},
            {"type": "event_msg", "payload": {"type": "user_message", "message": message}},
            {"type": "event_msg", "payload": {"type": "agent_message", "message": "done"}},
        ]
        path.write_text("\n".join(json.dumps(l) for l in lines) + "\n", encoding="utf-8")
        return path

    def test_lists_filters_and_previews_sessions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.write_session(root, "2026-10-14T09-00-00", "a" * 8 + "-1111-2222-3333-" + "b" * 12, "fix login")
            newer = self.write_session(root, "2026-10-14T10-30-00", "c" * 8 + "-1111-2222-3333-" + "d" * 12, "add tests\nmore")
            (root / "2026" / "10" / "14" / "notes.txt").write_text("ignored")
            sessions = ns.list_sessions(root)
            self.assertEqual([s.title for s in sessions], ["add tests", "fix login"])
            self.assertEqual(sessions[0].cwd, "/work/app")
            self.assertEqual(sessions[0].thread_id, "c" * 8 + "-1111-2222-3333-" + "d" * 12)
            self.assertEqual([s.title for s in ns.filter_sessions(sessions, "LOGIN")], ["fix login"])
            self.assertEqual(len(ns.filter_sessions(sessions, "2026-10-14 10:30")), 1)
            preview = ns.session_preview(newer)
            self.assertIn("You: add tests", preview)
            self.assertIn("Nez: done", preview)


class ConfigTests(unittest.TestCase):
    def test_set_top_level_replaces_and_inserts_before_tables(self):
        text = 'model = "old"\n# comment\n\n[mcp_servers.x]\nmodel = "nested"\n'
        updated = ns.set_top_level(text, "model", "qwen3.5:9b")
        self.assertIn('model = "qwen3.5:9b"', updated.splitlines()[0])
        self.assertIn('model = "nested"', updated)
        updated = ns.set_top_level(updated, "model_provider", "ollama")
        lines = updated.splitlines()
        self.assertLess(lines.index('model_provider = "ollama"'), lines.index("[mcp_servers.x]"))
        self.assertEqual(ns.read_top_level(updated), {"model": "qwen3.5:9b", "model_provider": "ollama"})

    def test_save_model_selection_writes_config_home(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(os.environ, {"NEZ_HOME": tmp}):
            self.assertEqual(ns.current_model(), ("ollama", ns.DEFAULT_LOCAL_MODEL))
            ns.save_model_selection("deepseek-chat", "deepseek")
            self.assertEqual(ns.current_model(), ("deepseek", "deepseek-chat"))
            self.assertNotIn("key", (Path(tmp) / "config.toml").read_text())

    def test_parse_show_reads_capabilities_and_context(self):
        caps = ns.parse_show({"capabilities": ["completion", "vision"],
                              "model_info": {"general.architecture": "qwen35", "qwen35.context_length": 262144}})
        self.assertTrue(caps.vision)
        self.assertFalse(caps.tools)
        self.assertEqual(caps.context_length, 262144)
        self.assertIn("unknown", ns.parse_show({}).vision_label())
        self.assertIn("text only", ns.parse_show({"capabilities": ["completion"]}).vision_label())


class BackupTests(unittest.TestCase):
    def test_backup_skips_heavy_folders_and_restores(self):
        with tempfile.TemporaryDirectory() as tmp:
            project = Path(tmp) / "app"
            (project / "src").mkdir(parents=True)
            (project / "src" / "main.py").write_text("print(1)\n")
            (project / "node_modules" / "dep").mkdir(parents=True)
            (project / "node_modules" / "dep" / "index.js").write_text("x")
            archive = ns.create_backup(project, Path(tmp) / "backups")
            with tarfile.open(archive) as tar:
                names = tar.getnames()
            self.assertIn("app/src/main.py", names)
            self.assertFalse(any("node_modules" in n for n in names))
            restore = Path(tmp) / "restore"
            restore.mkdir()
            ns.safe_extract(archive, restore)
            self.assertEqual((restore / "app" / "src" / "main.py").read_text(), "print(1)\n")

    def test_restore_refuses_path_traversal(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = Path(tmp) / "evil.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                data = b"owned"
                info = tarfile.TarInfo("../escape.txt")
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
            target = Path(tmp) / "target"
            target.mkdir()
            with self.assertRaises(ValueError):
                ns.safe_extract(archive, target)
            self.assertFalse((Path(tmp) / "escape.txt").exists())


class UpdateTests(unittest.TestCase):
    def test_version_comparison(self):
        self.assertTrue(ns.is_newer("v3.0.1", "nez-cli 3.0.0"))
        self.assertFalse(ns.is_newer("v3.0.0", "3.0.0"))
        self.assertFalse(ns.is_newer("garbage", "3.0.0"))

    def test_logs_pick_the_newest_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            old = Path(tmp) / "a.log"
            new = Path(tmp) / "b.log"
            old.write_text("old")
            new.write_text("x" * 10 + "tail")
            os.utime(old, (1, 1))
            self.assertEqual(ns.latest_log(Path(tmp)), new)
            self.assertEqual(ns.tail(new, max_bytes=4), "tail")


if __name__ == "__main__":
    unittest.main()
