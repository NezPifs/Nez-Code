"""Nez-Code dashboard.

A PyQt5 companion for the `nez` CLI. Chat runs `nez exec --json` as a child
process per message and resumes the same thread for follow-ups, so the
dashboard never needs its own copy of the agent loop.
"""

from __future__ import annotations

import os
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Callable, Optional

from PyQt5.QtCore import QBuffer, QIODevice, QObject, QProcess, QSize, Qt, QTimer, pyqtSignal
from PyQt5.QtGui import QIcon, QKeySequence, QPixmap
from PyQt5.QtWidgets import (
    QApplication,
    QCheckBox,
    QComboBox,
    QFileDialog,
    QFrame,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMainWindow,
    QMessageBox,
    QPlainTextEdit,
    QPushButton,
    QShortcut,
    QSplitter,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)

sys.path.insert(0, str(Path(__file__).resolve().parent))

import nez_support as ns  # noqa: E402

APP_NAME = "Nez-Code"
POLL_MS = 3000
UPDATE_CHECK_MS = 6 * 60 * 60 * 1000

BG = "#0c0b14"
PANEL = "#16152a"
BORDER = "#292642"
ACCENT = "#7c3aed"
TEXT = "#eceaf5"
DIM = "#a9a3c3"
GREEN = "#22c55e"
YELLOW = "#f59e0b"
RED = "#ef4444"

STYLE = f"""
QWidget {{ background: {BG}; color: {TEXT}; font-size: 13px; }}
QTabWidget::pane {{ border: 1px solid {BORDER}; }}
QTabBar::tab {{ background: {PANEL}; padding: 8px 16px; border: 1px solid {BORDER}; }}
QTabBar::tab:selected {{ background: {ACCENT}; }}
QPlainTextEdit, QLineEdit, QListWidget, QComboBox {{
    background: {PANEL}; border: 1px solid {BORDER}; border-radius: 6px; padding: 4px;
}}
QPushButton {{ background: {PANEL}; border: 1px solid {BORDER}; border-radius: 6px; padding: 6px 12px; }}
QPushButton:hover {{ border-color: {ACCENT}; }}
QPushButton:disabled {{ color: #5b5674; }}
QPushButton#primary {{ background: {ACCENT}; border-color: {ACCENT}; }}
QFrame#card {{ background: {PANEL}; border: 1px solid {BORDER}; border-radius: 8px; }}
"""


def nez_command() -> Optional[list[str]]:
    """The `nez` invocation: NEZ_BIN when set (a `.py` file runs under this
    interpreter, which the tests rely on), otherwise `nez` from PATH."""
    override = os.environ.get("NEZ_BIN")
    if override:
        return [sys.executable, override] if override.endswith(".py") else [override]
    found = ns.which("nez")
    return [found] if found else None


class Background(QObject):
    """Runs blocking calls off the UI thread and delivers results back to it."""

    done = pyqtSignal(object, object, object)  # callback, result, error

    def __init__(self) -> None:
        super().__init__()
        self.done.connect(self._deliver)

    def run(self, fn: Callable[[], Any], callback: Callable[[Any, Optional[Exception]], None]) -> None:
        def work() -> None:
            try:
                result, error = fn(), None
            except Exception as exc:  # noqa: BLE001 - surfaced to the UI
                result, error = None, exc
            self.done.emit(callback, result, error)

        threading.Thread(target=work, daemon=True).start()

    @staticmethod
    def _deliver(callback: Any, result: Any, error: Any) -> None:
        callback(result, error)


def card(title: str) -> tuple[QFrame, QLabel, QLabel]:
    frame = QFrame()
    frame.setObjectName("card")
    layout = QVBoxLayout(frame)
    heading = QLabel(title)
    heading.setStyleSheet(f"color: {DIM}; font-size: 11px; text-transform: uppercase;")
    value = QLabel("…")
    value.setStyleSheet("font-size: 18px; font-weight: 600;")
    detail = QLabel("")
    detail.setStyleSheet(f"color: {DIM};")
    detail.setWordWrap(True)
    layout.addWidget(heading)
    layout.addWidget(value)
    layout.addWidget(detail)
    frame.setProperty("detail", detail)
    return frame, value, detail


class PromptEdit(QPlainTextEdit):
    submit = pyqtSignal()

    def keyPressEvent(self, event: Any) -> None:  # noqa: N802 - Qt API
        if event.key() in (Qt.Key_Return, Qt.Key_Enter) and event.modifiers() & (
            Qt.ControlModifier | Qt.MetaModifier
        ):
            self.submit.emit()
            return
        super().keyPressEvent(event)


class ChatPanel(QWidget):
    thread_changed = pyqtSignal(str)

    def __init__(self, background: Background) -> None:
        super().__init__()
        self.background = background
        self.process: Optional[QProcess] = None
        self.stream = ns.ExecStream()
        self.thread_id: Optional[str] = None
        self.attachments: list[ns.ImageAttachment] = []
        self.started_at = 0.0
        self.pending_prompt = ""
        self._sent_attachments: list[ns.ImageAttachment] = []
        self.project_dir = Path.cwd()

        layout = QVBoxLayout(self)
        header = QHBoxLayout()
        self.session_label = QLabel("New conversation")
        self.session_label.setStyleSheet(f"color: {DIM};")
        self.vision_label = QLabel("")
        self.vision_label.setStyleSheet(f"color: {DIM};")
        header.addWidget(self.session_label)
        header.addStretch(1)
        header.addWidget(self.vision_label)
        layout.addLayout(header)

        self.transcript = QPlainTextEdit()
        self.transcript.setReadOnly(True)
        self.transcript.setMaximumBlockCount(6000)
        layout.addWidget(self.transcript, 1)

        self.thumbs = QListWidget()
        self.thumbs.setViewMode(QListWidget.IconMode)
        self.thumbs.setIconSize(QSize(72, 72))
        self.thumbs.setFixedHeight(100)
        self.thumbs.setVisible(False)
        layout.addWidget(self.thumbs)

        self.prompt = PromptEdit()
        self.prompt.setPlaceholderText("Ask Nez… (Ctrl+Enter to send)")
        self.prompt.setFixedHeight(110)
        self.prompt.submit.connect(self.send)
        layout.addWidget(self.prompt)

        buttons = QHBoxLayout()
        self.attach_btn = QPushButton("Attach image")
        self.paste_btn = QPushButton("Paste image")
        self.remove_btn = QPushButton("Remove image")
        self.new_btn = QPushButton("New conversation")
        self.export_btn = QPushButton("Export chat")
        self.status = QLabel("")
        self.status.setStyleSheet(f"color: {DIM};")
        self.stop_btn = QPushButton("Stop")
        self.send_btn = QPushButton("Send")
        self.send_btn.setObjectName("primary")
        for b in (self.attach_btn, self.paste_btn, self.remove_btn, self.new_btn, self.export_btn):
            buttons.addWidget(b)
        buttons.addWidget(self.status, 1)
        buttons.addWidget(self.stop_btn)
        buttons.addWidget(self.send_btn)
        layout.addLayout(buttons)

        self.attach_btn.clicked.connect(self.attach_images)
        self.paste_btn.clicked.connect(self.paste_image)
        self.remove_btn.clicked.connect(self.remove_image)
        self.new_btn.clicked.connect(self.new_conversation)
        self.export_btn.clicked.connect(self.export_chat)
        self.stop_btn.clicked.connect(self.stop)
        self.send_btn.clicked.connect(self.send)
        QShortcut(QKeySequence.Paste, self.prompt, activated=self._paste_shortcut)

        self.timer = QTimer(self)
        self.timer.timeout.connect(self._tick)
        self._set_running(False)

    # -- attachments -------------------------------------------------------

    def add_attachment(self, attachment: ns.ImageAttachment) -> None:
        if len(self.attachments) >= ns.MAX_IMAGES:
            attachment.cleanup()
            raise ValueError(f"Attach at most {ns.MAX_IMAGES} images per message.")
        self.attachments.append(attachment)
        item = QListWidgetItem(QIcon(QPixmap(str(attachment.path))), attachment.name)
        item.setToolTip(f"{attachment.name} ({attachment.size // 1024} KiB)")
        self.thumbs.addItem(item)
        self.thumbs.setVisible(True)

    def attach_images(self) -> None:
        paths, _ = QFileDialog.getOpenFileNames(
            self, "Attach images", str(self.project_dir), "Images (*.png *.jpg *.jpeg *.webp *.gif)"
        )
        for raw in paths:
            try:
                self.add_attachment(ns.attachment_from_file(Path(raw)))
            except (OSError, ValueError) as exc:
                QMessageBox.warning(self, APP_NAME, f"{Path(raw).name}: {exc}")
                break

    def paste_image(self) -> bool:
        image = QApplication.clipboard().image()
        if image.isNull():
            QMessageBox.information(self, APP_NAME, "The clipboard does not contain an image.")
            return False
        buffer = QBuffer()
        buffer.open(QIODevice.WriteOnly)
        image.save(buffer, "PNG")
        try:
            self.add_attachment(ns.attachment_from_bytes(bytes(buffer.data()), "pasted.png"))
        except ValueError as exc:
            QMessageBox.warning(self, APP_NAME, str(exc))
            return False
        return True

    def _paste_shortcut(self) -> None:
        if not QApplication.clipboard().image().isNull():
            self.paste_image()
        else:
            self.prompt.paste()

    def remove_image(self) -> None:
        row = self.thumbs.currentRow()
        if row < 0:
            row = len(self.attachments) - 1
        if row < 0:
            return
        self.thumbs.takeItem(row)
        self.attachments.pop(row).cleanup()
        self.thumbs.setVisible(bool(self.attachments))

    def clear_attachments(self, delete_files: bool) -> None:
        for attachment in self.attachments:
            if delete_files:
                attachment.cleanup()
        self.attachments = []
        self.thumbs.clear()
        self.thumbs.setVisible(False)

    # -- conversation ------------------------------------------------------

    def append(self, text: str) -> None:
        self.transcript.appendPlainText(text)
        self.transcript.verticalScrollBar().setValue(self.transcript.verticalScrollBar().maximum())

    def set_thread(self, thread_id: Optional[str], label: str = "") -> None:
        self.thread_id = thread_id
        if thread_id:
            self.session_label.setText(label or f"Session {thread_id[:8]}")
            self.thread_changed.emit(thread_id)
        else:
            self.session_label.setText("New conversation")

    def new_conversation(self) -> None:
        if self.is_running():
            QMessageBox.information(self, APP_NAME, "Stop the current response first.")
            return
        if self.transcript.toPlainText().strip():
            answer = QMessageBox.question(self, APP_NAME, "Start a new conversation? The current one stays in Sessions.")
            if answer != QMessageBox.Yes:
                return
        self.transcript.clear()
        self.clear_attachments(delete_files=True)
        self.set_thread(None)
        self.status.setText("")

    def resume(self, session: ns.SessionInfo) -> None:
        if self.is_running():
            QMessageBox.information(self, APP_NAME, "Stop the current response first.")
            return
        self.transcript.setPlainText(ns.session_preview(session.path))
        self.append("\n— resumed; new messages continue this session —\n")
        if session.cwd and Path(session.cwd).is_dir():
            self.project_dir = Path(session.cwd)
        self.set_thread(session.thread_id, session.label())

    def is_running(self) -> bool:
        return self.process is not None and self.process.state() != QProcess.NotRunning

    def send(self) -> None:
        if self.is_running():
            return
        command = nez_command()
        if not command:
            QMessageBox.warning(
                self, APP_NAME, "`nez` was not found on PATH. Install Nez-Code or set NEZ_BIN to the binary."
            )
            return
        text = self.prompt.toPlainText()
        try:
            args = ns.build_exec_args(ns.ChatRequest(prompt=text, images=list(self.attachments), thread_id=self.thread_id))
        except ValueError as exc:
            self.status.setText(str(exc))
            return
        self.pending_prompt = text
        self.stream = ns.ExecStream()
        self.process = QProcess(self)
        self.process.setWorkingDirectory(str(self.project_dir))
        self.process.readyReadStandardOutput.connect(self._read_stdout)
        self.process.readyReadStandardError.connect(self._read_stderr)
        self.process.finished.connect(self._finished)
        self.process.errorOccurred.connect(self._process_error)
        self.process.setProgram(command[0])
        self.process.setArguments(command[1:] + args)
        self.process.start()
        if not self.process.waitForStarted(5000):
            self.status.setText("Could not start nez; your message was kept.")
            self.process = None
            return
        names = ", ".join(a.name for a in self.attachments)
        self.append(f"You: {text.strip()}" + (f"\n[images: {names}]" if names else ""))
        self.prompt.clear()
        # The CLI reads attachments at startup; files are cleaned after it exits.
        self._sent_attachments = list(self.attachments)
        self.clear_attachments(delete_files=False)
        self.started_at = time.monotonic()
        self.timer.start(500)
        self._set_running(True)

    def stop(self) -> None:
        if self.is_running() and self.process is not None:
            self.process.terminate()
            if not self.process.waitForFinished(2000):
                self.process.kill()
            self.append("[stopped]")

    def _read_stdout(self) -> None:
        if self.process is None:
            return
        for out in self.stream.feed(bytes(self.process.readAllStandardOutput())):
            self._render(out)

    def _read_stderr(self) -> None:
        if self.process is None:
            return
        data = bytes(self.process.readAllStandardError()).decode("utf-8", errors="replace").strip()
        window = self.window()
        if data and hasattr(window, "log"):
            window.log(data)

    def _render(self, out: ns.StreamOutput) -> None:
        prefix = {"assistant": "Nez: ", "tool": "  › ", "reasoning": "  (thinking) ", "error": "[error] "}
        self.append(prefix.get(out.kind, "") + out.text)

    def _finished(self, code: int, _status: Any) -> None:
        for out in self.stream.feed(b"", final=True):
            self._render(out)
        if self.process is not None:
            leftover = bytes(self.process.readAllStandardOutput())
            for out in self.stream.feed(leftover, final=True):
                self._render(out)
        self.timer.stop()
        elapsed = ns.format_elapsed(time.monotonic() - self.started_at)
        usage = ns.format_usage(self.stream.usage)
        state = "failed" if (self.stream.failed or code != 0) else "done"
        self.status.setText(f"{state} in {elapsed}" + (f" · {usage}" if usage else ""))
        if self.stream.thread_id and self.stream.thread_id != self.thread_id:
            self.set_thread(self.stream.thread_id)
        for attachment in self._sent_attachments:
            attachment.cleanup()
        self._sent_attachments = []
        self._set_running(False)

    def _process_error(self, _error: Any) -> None:
        if self.process is not None and self.process.state() == QProcess.NotRunning:
            self.timer.stop()
            self._set_running(False)

    def _tick(self) -> None:
        self.status.setText("working… " + ns.format_elapsed(time.monotonic() - self.started_at))

    def _set_running(self, running: bool) -> None:
        self.send_btn.setEnabled(not running)
        self.stop_btn.setEnabled(running)
        for b in (self.attach_btn, self.paste_btn, self.remove_btn, self.new_btn):
            b.setEnabled(not running)

    def export_chat(self) -> None:
        path, _ = QFileDialog.getSaveFileName(self, "Export chat", "nez-chat.md", "Markdown (*.md);;Text (*.txt)")
        if path:
            Path(path).write_text(self.transcript.toPlainText() + "\n", encoding="utf-8")
            self.status.setText(f"Exported to {path}")


class ModelsPanel(QWidget):
    model_selected = pyqtSignal(str)

    def __init__(self, background: Background) -> None:
        super().__init__()
        self.background = background
        layout = QVBoxLayout(self)
        self.current = QLabel("")
        layout.addWidget(self.current)
        self.models = QListWidget()
        layout.addWidget(self.models, 1)
        self.capabilities = QLabel("")
        self.capabilities.setWordWrap(True)
        layout.addWidget(self.capabilities)
        row = QHBoxLayout()
        self.refresh_btn = QPushButton("Refresh")
        self.use_btn = QPushButton("Use selected model")
        self.use_btn.setObjectName("primary")
        row.addWidget(self.refresh_btn)
        row.addStretch(1)
        row.addWidget(self.use_btn)
        layout.addLayout(row)
        hint = QLabel(
            "Models come from your local Ollama server. Install more with `ollama pull <model>`. "
            "Vision support is read from Ollama's model metadata; set NEZ_VISION=on or off to override it in the CLI."
        )
        hint.setWordWrap(True)
        hint.setStyleSheet(f"color: {DIM};")
        layout.addWidget(hint)
        self.refresh_btn.clicked.connect(self.refresh)
        self.use_btn.clicked.connect(self.use_selected)
        self.models.currentTextChanged.connect(self.inspect)

    def refresh(self) -> None:
        provider, model = ns.current_model()
        self.current.setText(f"Current: {model or '(provider default)'} via {provider}")
        self.background.run(ns.ollama_models, self._loaded)

    def _loaded(self, models: Any, error: Optional[Exception]) -> None:
        self.models.clear()
        if error:
            self.capabilities.setText(f"Ollama is not reachable at {ns.ollama_url()}: {error}")
            return
        _, current = ns.current_model()
        for name in models:
            item = QListWidgetItem(name + ("  (current)" if name == current else ""))
            item.setData(Qt.UserRole, name)
            self.models.addItem(item)

    def selected(self) -> Optional[str]:
        item = self.models.currentItem()
        return item.data(Qt.UserRole) if item else None

    def inspect(self, _text: str) -> None:
        model = self.selected()
        if model:
            self.capabilities.setText(f"Checking {model}…")
            self.background.run(lambda: ns.ollama_capabilities(model), self._show_caps)

    def _show_caps(self, caps: Any, error: Optional[Exception]) -> None:
        if error:
            self.capabilities.setText(f"Could not read model details: {error}")
            return
        context = f"{caps.context_length:,} tokens" if caps.context_length else "unknown"
        tools = {True: "native", False: "text fallback", None: "unknown"}[caps.tools]
        self.capabilities.setText(f"{caps.vision_label()}\nTool calls: {tools}\nMaximum context: {context}")

    def use_selected(self) -> None:
        model = self.selected()
        if not model:
            return
        ns.save_model_selection(model, "ollama")
        self.refresh()
        self.model_selected.emit(model)


class SessionsPanel(QWidget):
    resume_requested = pyqtSignal(object)

    def __init__(self) -> None:
        super().__init__()
        self.sessions: list[ns.SessionInfo] = []
        layout = QVBoxLayout(self)
        self.filter = QLineEdit()
        self.filter.setPlaceholderText("Filter by title, date or folder")
        layout.addWidget(self.filter)
        split = QSplitter(Qt.Horizontal)
        self.list = QListWidget()
        self.preview = QPlainTextEdit()
        self.preview.setReadOnly(True)
        split.addWidget(self.list)
        split.addWidget(self.preview)
        split.setSizes([320, 600])
        layout.addWidget(split, 1)
        row = QHBoxLayout()
        self.refresh_btn = QPushButton("Refresh")
        self.resume_btn = QPushButton("Resume in chat")
        self.resume_btn.setObjectName("primary")
        row.addWidget(self.refresh_btn)
        row.addStretch(1)
        row.addWidget(self.resume_btn)
        layout.addLayout(row)
        self.filter.textChanged.connect(self.apply_filter)
        self.list.currentRowChanged.connect(self.show_preview)
        self.refresh_btn.clicked.connect(self.refresh)
        self.resume_btn.clicked.connect(self.resume)

    def refresh(self) -> None:
        self.sessions = ns.list_sessions()
        self.apply_filter()

    def visible(self) -> list[ns.SessionInfo]:
        return ns.filter_sessions(self.sessions, self.filter.text())

    def apply_filter(self, _text: str = "") -> None:
        self.list.clear()
        for session in self.visible():
            self.list.addItem(session.label())

    def show_preview(self, row: int) -> None:
        shown = self.visible()
        if 0 <= row < len(shown):
            self.preview.setPlainText(ns.session_preview(shown[row].path))

    def resume(self) -> None:
        shown = self.visible()
        row = self.list.currentRow()
        if 0 <= row < len(shown):
            self.resume_requested.emit(shown[row])


class ProvidersPanel(QWidget):
    def __init__(self) -> None:
        super().__init__()
        layout = QVBoxLayout(self)
        intro = QLabel(
            "Local Ollama needs no key. Hosted providers keep their API key in your operating system's "
            "keychain; the dashboard passes it to `nez login` and never writes it to disk."
        )
        intro.setWordWrap(True)
        layout.addWidget(intro)
        grid = QGridLayout()
        self.provider = QComboBox()
        self.provider.addItem("Ollama (local)", "ollama")
        self.provider.addItem("DeepSeek", "deepseek")
        self.model = QLineEdit()
        self.key = QLineEdit()
        self.key.setEchoMode(QLineEdit.Password)
        self.key.setPlaceholderText("API key (DeepSeek only)")
        grid.addWidget(QLabel("Provider"), 0, 0)
        grid.addWidget(self.provider, 0, 1)
        grid.addWidget(QLabel("Model"), 1, 0)
        grid.addWidget(self.model, 1, 1)
        grid.addWidget(QLabel("API key"), 2, 0)
        grid.addWidget(self.key, 2, 1)
        layout.addLayout(grid)
        self.save_btn = QPushButton("Save provider")
        self.save_btn.setObjectName("primary")
        self.result = QLabel("")
        self.result.setWordWrap(True)
        layout.addWidget(self.save_btn)
        layout.addWidget(self.result)
        layout.addStretch(1)
        self.provider.currentIndexChanged.connect(self._provider_changed)
        self.save_btn.clicked.connect(self.save)
        self.load()

    def load(self) -> None:
        provider, model = ns.current_model()
        index = max(0, self.provider.findData(provider))
        self.provider.setCurrentIndex(index)
        self.model.setText(model)
        self._provider_changed()

    def _provider_changed(self) -> None:
        hosted = self.provider.currentData() == "deepseek"
        self.key.setEnabled(hosted)
        if hosted and not self.model.text().strip().startswith("deepseek"):
            self.model.setText("deepseek-chat")
        if not hosted and self.model.text().startswith("deepseek-chat"):
            self.model.setText(ns.DEFAULT_LOCAL_MODEL)

    def save(self) -> None:
        provider = self.provider.currentData()
        model = self.model.text().strip() or (
            "deepseek-chat" if provider == "deepseek" else ns.DEFAULT_LOCAL_MODEL
        )
        key = self.key.text().strip()
        if key:
            command = nez_command()
            if not command:
                self.result.setText("`nez` was not found on PATH, so the key could not be stored.")
                return
            completed = subprocess.run(
                [*command, "login", "--provider", provider, "--with-api-key"],
                input=key,
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.key.clear()
            if completed.returncode != 0:
                self.result.setText((completed.stderr or completed.stdout).strip() or "Storing the key failed.")
                return
        ns.save_model_selection(model, provider)
        self.result.setText(f"Saved: {model} via {provider}.")


class LogsPanel(QWidget):
    def __init__(self) -> None:
        super().__init__()
        layout = QVBoxLayout(self)
        self.path = QLabel("")
        self.view = QPlainTextEdit()
        self.view.setReadOnly(True)
        self.dashboard_log = QPlainTextEdit()
        self.dashboard_log.setReadOnly(True)
        self.dashboard_log.setMaximumBlockCount(2000)
        self.refresh_btn = QPushButton("Refresh")
        layout.addWidget(self.path)
        layout.addWidget(self.view, 3)
        layout.addWidget(QLabel("Dashboard process output"))
        layout.addWidget(self.dashboard_log, 1)
        layout.addWidget(self.refresh_btn)
        self.refresh_btn.clicked.connect(self.refresh)

    def refresh(self) -> None:
        path = ns.latest_log()
        if path is None:
            self.path.setText(f"No logs yet in {ns.log_dir()}")
            self.view.clear()
            return
        self.path.setText(str(path))
        self.view.setPlainText(ns.tail(path))
        self.view.verticalScrollBar().setValue(self.view.verticalScrollBar().maximum())


class BackupsPanel(QWidget):
    def __init__(self) -> None:
        super().__init__()
        layout = QVBoxLayout(self)
        row = QHBoxLayout()
        self.project = QLineEdit(str(Path.cwd()))
        browse = QPushButton("Browse…")
        row.addWidget(QLabel("Project"))
        row.addWidget(self.project, 1)
        row.addWidget(browse)
        layout.addLayout(row)
        self.list = QListWidget()
        layout.addWidget(self.list, 1)
        buttons = QHBoxLayout()
        self.create_btn = QPushButton("Create backup")
        self.create_btn.setObjectName("primary")
        self.restore_btn = QPushButton("Restore selected…")
        buttons.addWidget(self.create_btn)
        buttons.addWidget(self.restore_btn)
        buttons.addStretch(1)
        layout.addLayout(buttons)
        self.result = QLabel("")
        layout.addWidget(self.result)
        browse.clicked.connect(self.browse)
        self.create_btn.clicked.connect(self.create)
        self.restore_btn.clicked.connect(self.restore)

    def browse(self) -> None:
        path = QFileDialog.getExistingDirectory(self, "Project folder", self.project.text())
        if path:
            self.project.setText(path)

    def refresh(self) -> None:
        self.list.clear()
        for path in ns.list_backups():
            self.list.addItem(path.name)

    def create(self) -> None:
        project = Path(self.project.text()).expanduser()
        if not project.is_dir():
            self.result.setText("Choose an existing project folder.")
            return
        archive = ns.create_backup(project)
        self.result.setText(f"Created {archive}")
        self.refresh()

    def restore(self) -> None:
        item = self.list.currentItem()
        if item is None:
            return
        archive = ns.backups_dir() / item.text()
        target = QFileDialog.getExistingDirectory(self, "Restore into", str(Path.home()))
        if not target:
            return
        if QMessageBox.question(self, APP_NAME, f"Extract {archive.name} into {target}?") != QMessageBox.Yes:
            return
        try:
            ns.safe_extract(archive, Path(target))
            self.result.setText(f"Restored into {target}")
        except (OSError, ValueError) as exc:
            self.result.setText(f"Restore failed: {exc}")


class OverviewPanel(QWidget):
    def __init__(self, background: Background) -> None:
        super().__init__()
        self.background = background
        self.latest: Optional[str] = None
        layout = QVBoxLayout(self)
        banner = QLabel(f"{APP_NAME} — local first")
        banner.setStyleSheet("font-size: 22px; font-weight: 700;")
        layout.addWidget(banner)
        grid = QGridLayout()
        self.cards: dict[str, tuple[QLabel, QLabel]] = {}
        for i, name in enumerate(["Model", "Provider", "Loaded", "Sessions", "Version", "Updates"]):
            frame, value, detail = card(name)
            self.cards[name] = (value, detail)
            grid.addWidget(frame, i // 3, i % 3)
        layout.addLayout(grid)
        row = QHBoxLayout()
        self.check_btn = QPushButton("Check for updates")
        self.apply_btn = QPushButton("Apply update")
        self.apply_btn.setObjectName("primary")
        self.revert_btn = QPushButton("Revert last update")
        self.auto_check = QCheckBox("Check automatically (updates are never applied without a click)")
        row.addWidget(self.check_btn)
        row.addWidget(self.apply_btn)
        row.addWidget(self.revert_btn)
        row.addStretch(1)
        layout.addLayout(row)
        layout.addWidget(self.auto_check)
        self.update_log = QPlainTextEdit()
        self.update_log.setReadOnly(True)
        self.update_log.setMaximumBlockCount(500)
        layout.addWidget(self.update_log, 1)
        self.apply_btn.setEnabled(False)
        state = ns.load_dashboard_state()
        self.auto_check.setChecked(bool(state.get("auto_check_updates", True)))
        self.auto_check.toggled.connect(self._auto_toggled)
        self.check_btn.clicked.connect(self.check_updates)
        self.apply_btn.clicked.connect(lambda: self._run_updater([]))
        self.revert_btn.clicked.connect(self.revert)
        self.updater: Optional[QProcess] = None

    def set_card(self, name: str, value: str, detail: str = "", color: str = TEXT) -> None:
        label, detail_label = self.cards[name]
        label.setText(value)
        label.setStyleSheet(f"font-size: 18px; font-weight: 600; color: {color};")
        detail_label.setText(detail)

    def refresh(self) -> None:
        provider, model = ns.current_model()
        self.set_card("Model", model or "(default)")
        self.set_card("Provider", provider)
        self.set_card("Sessions", str(len(ns.list_sessions())), str(ns.sessions_dir()))
        self.set_card("Version", self.version())
        self.background.run(ns.ollama_loaded, self._loaded)

    def _loaded(self, models: Any, error: Optional[Exception]) -> None:
        if error:
            self.set_card("Loaded", "Ollama offline", str(error), RED)
            return
        if not models:
            self.set_card("Loaded", "none", "No model is loaded right now.", DIM)
            return
        first = models[0]
        vram = first.get("size_vram") or 0
        size = first.get("size") or 0
        share = f"{100 * vram // size}% on GPU" if size else ""
        self.set_card("Loaded", first.get("name", "?"), share, GREEN)

    def version(self) -> str:
        command = nez_command()
        if not command:
            return "not installed"
        try:
            out = subprocess.run([*command, "--version"], capture_output=True, text=True, timeout=10).stdout
        except (OSError, subprocess.SubprocessError):
            return "unknown"
        parsed = ns.parse_version(out)
        return ".".join(map(str, parsed)) if parsed else out.strip() or "unknown"

    def _auto_toggled(self, checked: bool) -> None:
        state = ns.load_dashboard_state()
        state["auto_check_updates"] = checked
        ns.save_dashboard_state(state)

    def check_updates(self) -> None:
        self.set_card("Updates", "checking…")
        self.background.run(ns.latest_release, self._release)

    def _release(self, release: Any, error: Optional[Exception]) -> None:
        if error:
            self.set_card("Updates", "check failed", str(error), YELLOW)
            return
        tag = release.get("tag_name", "")
        current = self.version()
        if ns.is_newer(tag, current):
            self.latest = tag
            self.set_card("Updates", f"{tag} available", "Click Apply update to install it.", YELLOW)
            self.apply_btn.setEnabled(True)
        else:
            self.set_card("Updates", "up to date", f"Latest release: {tag or 'none'}", GREEN)
            self.apply_btn.setEnabled(False)

    def revert(self) -> None:
        if QMessageBox.question(self, APP_NAME, "Restore the Nez-Code version from before the last update?") == QMessageBox.Yes:
            self._run_updater(["--rollback"])

    def _run_updater(self, extra: list[str]) -> None:
        command = nez_command()
        if not command or (self.updater and self.updater.state() != QProcess.NotRunning):
            return
        self.updater = QProcess(self)
        self.updater.setProcessChannelMode(QProcess.MergedChannels)
        self.updater.readyReadStandardOutput.connect(
            lambda: self.update_log.appendPlainText(
                bytes(self.updater.readAllStandardOutput()).decode("utf-8", errors="replace").rstrip()
            )
        )
        self.updater.finished.connect(lambda code, _s: self._updater_done(code))
        self.updater.start(command[0], [*command[1:], "update", "--yes", *extra])

    def _updater_done(self, code: int) -> None:
        self.update_log.appendPlainText(f"updater exited with code {code}")
        self.refresh()
        self.check_updates()


class Dashboard(QMainWindow):
    def __init__(self) -> None:
        super().__init__()
        self.setWindowTitle(f"{APP_NAME} Dashboard")
        self.resize(1100, 760)
        self.background = Background()
        self.tabs = QTabWidget()
        self.overview = OverviewPanel(self.background)
        self.chat = ChatPanel(self.background)
        self.models = ModelsPanel(self.background)
        self.sessions = SessionsPanel()
        self.providers = ProvidersPanel()
        self.logs = LogsPanel()
        self.backups = BackupsPanel()
        for name, widget in [
            ("Dashboard", self.overview),
            ("Chat", self.chat),
            ("Models", self.models),
            ("Sessions", self.sessions),
            ("Providers", self.providers),
            ("Logs", self.logs),
            ("Backups", self.backups),
        ]:
            self.tabs.addTab(widget, name)
        self.setCentralWidget(self.tabs)
        self.sessions.resume_requested.connect(self._resume)
        self.models.model_selected.connect(lambda _m: self._refresh_vision())
        self.tabs.currentChanged.connect(self._tab_changed)
        self.poll = QTimer(self)
        self.poll.timeout.connect(self.overview.refresh)
        self.poll.start(POLL_MS)
        self.update_timer = QTimer(self)
        self.update_timer.timeout.connect(self._auto_update_check)
        self.update_timer.start(UPDATE_CHECK_MS)
        self.overview.refresh()
        self._refresh_vision()
        QTimer.singleShot(5000, self._auto_update_check)

    def log(self, text: str) -> None:
        self.logs.dashboard_log.appendPlainText(text)

    def _auto_update_check(self) -> None:
        if self.overview.auto_check.isChecked():
            self.overview.check_updates()

    def _refresh_vision(self) -> None:
        provider, model = ns.current_model()
        if provider != "ollama" or not model:
            self.chat.vision_label.setText("Vision: checked by the provider when you send")
            return
        self.background.run(
            lambda: ns.ollama_capabilities(model),
            lambda caps, err: self.chat.vision_label.setText(
                caps.vision_label() if caps else "Vision: Ollama unreachable"
            ),
        )

    def _resume(self, session: ns.SessionInfo) -> None:
        self.chat.resume(session)
        self.tabs.setCurrentWidget(self.chat)

    def _tab_changed(self, index: int) -> None:
        widget = self.tabs.widget(index)
        if widget is self.models:
            self.models.refresh()
        elif widget is self.sessions:
            self.sessions.refresh()
        elif widget is self.logs:
            self.logs.refresh()
        elif widget is self.backups:
            self.backups.refresh()

    def closeEvent(self, event: Any) -> None:  # noqa: N802 - Qt API
        if self.chat.is_running():
            self.chat.stop()
        event.accept()


def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName(APP_NAME)
    app.setStyleSheet(STYLE)
    window = Dashboard()
    window.show()
    return app.exec_()


if __name__ == "__main__":
    raise SystemExit(main())
