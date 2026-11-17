"""Widget tests that run real child processes against a scripted `nez`."""

import json
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from PyQt5.QtWidgets import QApplication, QMessageBox  # noqa: E402

import nez_dashboard as nd  # noqa: E402
import nez_support as ns  # noqa: E402

APP = QApplication.instance() or QApplication([])
# A valid 1x1 RGB PNG.
PNG = b'\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde\x00\x00\x00\x0cIDATx\x9cc\xf8\xcf\xc0\x00\x00\x03\x01\x01\x00\xc9\xfe\x92\xef\x00\x00\x00\x00IEND\xaeB`\x82'


def wait_until(predicate, timeout=15.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        APP.processEvents()
        if predicate():
            return True
        time.sleep(0.02)
    return False


class DashboardTestCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.home = Path(self.tmp.name) / "home"
        self.home.mkdir()
        self.log = Path(self.tmp.name) / "calls.jsonl"
        self.env = mock.patch.dict(os.environ, {
            "NEZ_HOME": str(self.home),
            "NEZ_BIN": str(HERE / "testdata" / "fake_nez.py"),
            "FAKE_NEZ_LOG": str(self.log),
        })
        self.env.start()
        # Network lookups are replaced so tests never need Ollama.
        self.patches = [
            mock.patch.object(ns, "ollama_models", return_value=["qwen3.5:9b", "llava:7b"]),
            mock.patch.object(ns, "ollama_loaded", return_value=[]),
            mock.patch.object(ns, "ollama_capabilities",
                              return_value=ns.ModelCapabilities(vision=True, tools=True, context_length=32768)),
            mock.patch.object(ns, "latest_release", return_value={"tag_name": "v3.0.1"}),
        ]
        for p in self.patches:
            p.start()

    def tearDown(self):
        for p in self.patches:
            p.stop()
        self.env.stop()
        self.tmp.cleanup()

    def calls(self):
        if not self.log.exists():
            return []
        return [json.loads(l) for l in self.log.read_text().splitlines() if l.strip()]


class ChatTests(DashboardTestCase):
    def test_conversation_continues_with_resume_and_reports_usage(self):
        chat = nd.ChatPanel(nd.Background())
        chat.project_dir = Path(self.tmp.name)
        chat.prompt.setPlainText("list the files")
        chat.send()
        self.assertTrue(wait_until(lambda: not chat.is_running() and "first answer" in chat.transcript.toPlainText()))
        self.assertEqual(chat.thread_id, "11111111-2222-3333-4444-555555555555")
        self.assertIn("1,200 in / 34 out", chat.status.text())
        self.assertIn("$ ls", chat.transcript.toPlainText())
        self.assertEqual(chat.prompt.toPlainText(), "")

        chat.prompt.setPlainText("and now?")
        chat.send()
        self.assertTrue(wait_until(lambda: not chat.is_running() and "résumé ok" in chat.transcript.toPlainText()))
        exec_calls = [c for c in self.calls() if c[:1] == ["exec"]]
        self.assertEqual(exec_calls[0][:2], ["exec", "--json"])
        self.assertEqual(exec_calls[1][:3], ["exec", "resume", chat.thread_id])
        self.assertEqual(exec_calls[1][-1], "and now?")

    def test_images_are_passed_to_the_cli_and_temp_files_removed(self):
        chat = nd.ChatPanel(nd.Background())
        chat.project_dir = Path(self.tmp.name)
        pasted = ns.attachment_from_bytes(PNG, "pasted.png")
        chat.add_attachment(pasted)
        self.assertTrue(chat.thumbs.isVisible() or chat.thumbs.count() == 1)
        chat.send()
        self.assertTrue(wait_until(lambda: not chat.is_running() and "first answer" in chat.transcript.toPlainText()))
        args = [c for c in self.calls() if c[:1] == ["exec"]][0]
        self.assertEqual(args[args.index("--image") + 1], str(pasted.path))
        self.assertEqual(args[-1], "Describe these images.")
        self.assertFalse(pasted.path.exists())
        self.assertEqual(chat.thumbs.count(), 0)

    def test_attachment_limit_is_enforced(self):
        chat = nd.ChatPanel(nd.Background())
        for _ in range(ns.MAX_IMAGES):
            chat.add_attachment(ns.attachment_from_bytes(PNG, "p.png"))
        extra = ns.attachment_from_bytes(PNG, "p.png")
        with self.assertRaises(ValueError):
            chat.add_attachment(extra)
        self.assertFalse(extra.path.exists())
        chat.clear_attachments(delete_files=True)

    def test_stop_ends_a_running_turn(self):
        with mock.patch.dict(os.environ, {"FAKE_NEZ_SLEEP": "30"}):
            chat = nd.ChatPanel(nd.Background())
            chat.project_dir = Path(self.tmp.name)
            chat.prompt.setPlainText("slow")
            chat.send()
            self.assertTrue(wait_until(chat.is_running, 5))
            self.assertTrue(chat.stop_btn.isEnabled())
            chat.stop()
            self.assertTrue(wait_until(lambda: not chat.is_running(), 10))
            self.assertIn("[stopped]", chat.transcript.toPlainText())
            self.assertTrue(chat.send_btn.isEnabled())

    def test_missing_cli_keeps_the_prompt(self):
        with mock.patch.dict(os.environ, {"NEZ_BIN": str(Path(self.tmp.name) / "missing")}):
            chat = nd.ChatPanel(nd.Background())
            chat.prompt.setPlainText("keep me")
            chat.send()
            wait_until(lambda: False, 0.5)
            self.assertEqual(chat.prompt.toPlainText(), "keep me")
            self.assertFalse(chat.is_running())

    def test_export_writes_the_transcript(self):
        chat = nd.ChatPanel(nd.Background())
        chat.append("You: hi")
        target = Path(self.tmp.name) / "chat.md"
        with mock.patch.object(nd.QFileDialog, "getSaveFileName", return_value=(str(target), "")):
            chat.export_chat()
        self.assertEqual(target.read_text(), "You: hi\n")


class PanelTests(DashboardTestCase):
    def test_model_panel_lists_models_and_saves_the_selection(self):
        panel = nd.ModelsPanel(nd.Background())
        panel.refresh()
        self.assertTrue(wait_until(lambda: panel.models.count() == 2))
        panel.models.setCurrentRow(1)
        self.assertTrue(wait_until(lambda: "Vision: supported" in panel.capabilities.text()))
        panel.use_selected()
        self.assertEqual(ns.current_model(), ("ollama", "llava:7b"))

    def test_sessions_resume_into_chat(self):
        day = self.home / "sessions" / "2026" / "10" / "14"
        day.mkdir(parents=True)
        thread = "abcdefab-1111-2222-3333-444455556666"
        (day / f"rollout-2026-10-14T08-00-00-{thread}.jsonl").write_text(
            json.dumps({"type": "session_meta", "payload": {"cwd": self.tmp.name}}) + "\n"
            + json.dumps({"type": "event_msg", "payload": {"type": "user_message", "message": "old task"}}) + "\n"
        )
        window = nd.Dashboard()
        window.sessions.refresh()
        self.assertEqual(window.sessions.list.count(), 1)
        window.sessions.list.setCurrentRow(0)
        self.assertIn("old task", window.sessions.preview.toPlainText())
        window.sessions.resume()
        self.assertEqual(window.chat.thread_id, thread)
        self.assertIs(window.tabs.currentWidget(), window.chat)
        window.close()

    def test_provider_key_goes_to_nez_login_not_disk(self):
        panel = nd.ProvidersPanel()
        panel.provider.setCurrentIndex(panel.provider.findData("deepseek"))
        panel.key.setText("sk-secret-value")
        panel.save()
        self.assertEqual(panel.key.text(), "")
        self.assertIn(["login", "--provider", "deepseek", "--with-api-key"], self.calls())
        config = (self.home / "config.toml").read_text()
        self.assertIn('model_provider = "deepseek"', config)
        self.assertNotIn("sk-secret-value", config)

    def test_overview_reports_version_and_available_update(self):
        panel = nd.OverviewPanel(nd.Background())
        self.assertEqual(panel.version(), "3.0.0")
        panel.check_updates()
        self.assertTrue(wait_until(lambda: "available" in panel.cards["Updates"][0].text()))
        self.assertTrue(panel.apply_btn.isEnabled())

    def test_update_apply_is_manual_and_revert_asks_first(self):
        panel = nd.OverviewPanel(nd.Background())
        with mock.patch.object(nd.QMessageBox, "question", return_value=QMessageBox.No):
            panel.revert()
        self.assertFalse(any(c[:1] == ["update"] for c in self.calls()))
        with mock.patch.object(nd.QMessageBox, "question", return_value=QMessageBox.Yes):
            panel.revert()
        self.assertTrue(wait_until(lambda: ["update", "--yes", "--rollback"] in self.calls()))
        self.assertTrue(wait_until(lambda: "updater exited" in panel.update_log.toPlainText()))


if __name__ == "__main__":
    unittest.main()
