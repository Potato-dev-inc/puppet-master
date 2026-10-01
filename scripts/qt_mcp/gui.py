"""Button-first PySide6 dashboard for operating Puppet Master over local MCP."""

from __future__ import annotations

import json
import os
import sys
import uuid
from datetime import datetime
from pathlib import Path
from typing import Any

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QApplication,
    QButtonGroup,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QGridLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QListWidget,
    QListWidgetItem,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QPlainTextEdit,
    QScrollArea,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)

try:
    from transport import McpTransport
    from forms import ask_tool_arguments, render_result
except ImportError:
    from .transport import McpTransport
    from .forms import ask_tool_arguments, render_result


REPO_ROOT = Path(__file__).resolve().parents[2]
TERMINAL_STATES = ["waiting_input", "completed", "failed", "cancelled"]
BUTTON_LABELS = {
    "bridge_health": "Check bridge",
    "list_panes": "List workers",
    "delegate_work": "Delegate task",
    "get_operation": "Get operation status",
    "wait_for_operation": "Wait for update",
    "cancel_operation": "Cancel operation",
    "run_agent": "Run agent",
    "wait_agents": "Wait for agents",
    "cancel_agent": "Cancel agent",
    "list_agents": "List agents",
    "agent_transcript": "Agent transcript",
    "take_over": "Take over worker",
    "set_mode": "Set tool mode",
    "send_agent": "Send follow-up",
    "answer_prompt": "Answer prompt",
}
AGENT_CATALOG_TOOLS = {
    "run_agent", "wait_agents", "cancel_agent", "list_agents", "agent_transcript", "take_over",
}
MODES = ("agent", "shell", "both")


def default_command() -> tuple[str, list[str]]:
    binary_name = "puppet-master-mcp.exe" if os.name == "nt" else "puppet-master-mcp"
    updated_name = "puppet-master-mcp-updated.exe" if os.name == "nt" else "puppet-master-mcp-updated"
    debug_dir = REPO_ROOT / "packages/app/src-tauri/target/debug"
    available = [path for path in (debug_dir / binary_name, debug_dir / updated_name) if path.is_file()]
    if available:
        return str(max(available, key=lambda path: path.stat().st_mtime_ns)), []
    for candidate in (
        REPO_ROOT / "packages/mcp-server/dist" / binary_name,
    ):
        if candidate.is_file():
            return str(candidate), []
    launcher = REPO_ROOT / "packages/mcp-server/dist/index.js"
    if launcher.is_file():
        return "node", [str(launcher)]
    return binary_name, []


def operation_snapshot(value: Any) -> dict[str, Any] | None:
    """Find an operation in structured MCP content, progress, or text output."""
    pending = [value]
    visited: set[int] = set()
    while pending:
        current = pending.pop(0)
        if not isinstance(current, dict) or id(current) in visited:
            continue
        visited.add(id(current))
        if current.get("isError") is True:
            continue
        if "operation_id" in current:
            return current
        for key in ("structuredContent", "snapshot", "message"):
            nested = current.get(key)
            if isinstance(nested, dict):
                pending.append(nested)
            elif isinstance(nested, str):
                try:
                    parsed = json.loads(nested)
                except (ValueError, TypeError):
                    continue
                if isinstance(parsed, dict):
                    pending.append(parsed)
        content = current.get("content")
        if isinstance(content, list):
            for item in content:
                if isinstance(item, dict) and isinstance(item.get("text"), str):
                    try:
                        parsed = json.loads(item["text"])
                    except (ValueError, TypeError):
                        continue
                    if isinstance(parsed, dict):
                        pending.append(parsed)
    return None


def agent_runs(value: Any) -> list[dict[str, Any]]:
    """Extract structured agent records from MCP results and progress payloads."""
    found: dict[str, dict[str, Any]] = {}
    pending = [value]
    visited: set[int] = set()
    while pending:
        current = pending.pop(0)
        if isinstance(current, list):
            pending.extend(current)
            continue
        if not isinstance(current, dict) or id(current) in visited:
            continue
        visited.add(id(current))
        if current.get("isError") is True:
            continue
        handle = current.get("handle")
        if isinstance(handle, str) and handle and ("status" in current or "operation_id" in current):
            found[handle] = current
        for key in ("structuredContent", "agent", "agents", "message", "data"):
            nested = current.get(key)
            if isinstance(nested, (dict, list)):
                pending.append(nested)
            elif isinstance(nested, str):
                try:
                    parsed = json.loads(nested)
                except (ValueError, TypeError):
                    continue
                if isinstance(parsed, (dict, list)):
                    pending.append(parsed)
        content = current.get("content")
        if isinstance(content, list):
            for item in content:
                if isinstance(item, dict) and isinstance(item.get("text"), str):
                    try:
                        parsed = json.loads(item["text"])
                    except (ValueError, TypeError):
                        continue
                    if isinstance(parsed, (dict, list)):
                        pending.append(parsed)
    return list(found.values())


def _is_pane(item: Any) -> bool:
    return (
        isinstance(item, dict)
        and bool(item.get("pane_id") or item.get("id"))
        and ("agent_type" in item or "agent" in item)
    )


def _pane_id(pane: dict[str, Any]) -> str:
    return str(pane.get("pane_id") or pane.get("id") or "")


def _pane_label(pane: dict[str, Any]) -> str:
    pane_id = _pane_id(pane)
    short = pane_id[:8] if pane_id else "unknown"
    agent = str(pane.get("agent_type") or pane.get("agent") or "worker")
    status = str(pane.get("status") or "")
    cwd = str(pane.get("cwd") or pane.get("working_directory") or "")
    cwd_name = Path(cwd).name if cwd else ""
    parts = [agent, short]
    if status:
        parts.append(status)
    if cwd_name:
        parts.append(cwd_name)
    return " · ".join(parts)


def _extract_panes(value: Any) -> list[dict[str, Any]]:
    """Collect every worker pane in an MCP result, not just the first nested list."""
    found: list[dict[str, Any]] = []
    seen: set[str] = set()
    pending: list[Any] = [value]
    visited: set[int] = set()
    while pending:
        current = pending.pop(0)
        identity = id(current)
        if identity in visited:
            continue
        if isinstance(current, dict):
            visited.add(identity)
            if _is_pane(current):
                pane_id = _pane_id(current)
                if pane_id.startswith("puppet-master-orchestrator-"):
                    continue
                if pane_id and pane_id not in seen:
                    seen.add(pane_id)
                    found.append(current)
                continue
            pending.extend(current.values())
            continue
        if isinstance(current, list):
            pending.extend(current)
            continue
        if isinstance(current, str):
            try:
                pending.append(json.loads(current))
            except (ValueError, TypeError):
                continue
    return found


def ask_worker_pane(parent: QWidget | None, panes: list[dict[str, Any]]) -> dict[str, Any] | None:
    dialog = QDialog(parent)
    dialog.setWindowTitle("Choose worker")
    dialog.setMinimumWidth(520)
    dialog.setMinimumHeight(280)
    layout = QVBoxLayout(dialog)
    layout.addWidget(QLabel("Worker panes"))
    listing = QListWidget()
    for pane in panes:
        item = QListWidgetItem(_pane_label(pane))
        item.setToolTip(_pane_id(pane))
        item.setData(Qt.ItemDataRole.UserRole, pane)
        listing.addItem(item)
    if listing.count():
        listing.setCurrentRow(0)
    listing.itemDoubleClicked.connect(dialog.accept)
    layout.addWidget(listing, 1)
    buttons = QDialogButtonBox(
        QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
    )
    buttons.accepted.connect(dialog.accept)
    buttons.rejected.connect(dialog.reject)
    layout.addWidget(buttons)
    if dialog.exec() != QDialog.DialogCode.Accepted:
        return None
    item = listing.currentItem()
    if item is None:
        return None
    chosen = item.data(Qt.ItemDataRole.UserRole)
    return chosen if isinstance(chosen, dict) else None


class MainWindow(QMainWindow):
    def __init__(self) -> None:
        super().__init__()
        self.setWindowTitle("Puppet Master · MCP Console")
        self.resize(950, 700)
        self.transport = McpTransport(self)
        self._connected = False
        self._active_requests: dict[int, str] = {}
        self._tools: dict[str, dict[str, Any]] = {}
        self.tool_buttons: dict[str, QPushButton] = {}
        self._pane_cache: list[dict[str, Any]] = []
        self._project_path = str(REPO_ROOT)
        self._pane_id = ""
        self._agent_type = "codex"
        self._agent_handle = ""
        self._agent_revision = 0
        self._agent_cache: dict[str, dict[str, Any]] = {}
        self._mode = "agent"
        self._operation_id = ""
        self._revision = 0
        self._idempotency_keys: dict[str, str] = {}
        self._build_ui()
        self._connect_transport()
        self._apply_style()

    def _build_ui(self) -> None:
        central = QWidget()
        root = QVBoxLayout(central)
        root.setContentsMargins(18, 15, 18, 16)
        root.setSpacing(12)

        top = QHBoxLayout()
        brand = QVBoxLayout()
        title = QLabel("Puppet Master")
        title.setObjectName("title")
        subtitle = QLabel("Local MCP operator")
        subtitle.setObjectName("subtitle")
        brand.addWidget(title)
        brand.addWidget(subtitle)
        top.addLayout(brand)
        top.addStretch(1)
        self.connection_status = QLabel("Disconnected")
        self.connection_status.setObjectName("connection")
        top.addWidget(self.connection_status)
        root.addLayout(top)

        conn_group = QGroupBox("Session")
        conn = QHBoxLayout(conn_group)
        self.connect_button = QPushButton("Connect")
        self.connect_button.setObjectName("primary")
        self.disconnect_button = QPushButton("Disconnect")
        self.disconnect_button.setEnabled(False)
        self.project_button = QPushButton("Choose project")
        self.worker_button = QPushButton("Choose worker")
        self.project_label = QLabel(Path(self._project_path).name)
        self.project_label.setToolTip(self._project_path)
        self.worker_label = QLabel("No worker selected")
        conn.addWidget(self.connect_button)
        conn.addWidget(self.disconnect_button)
        conn.addWidget(self.project_button)
        conn.addWidget(self.project_label, 1)
        conn.addWidget(self.worker_button)
        conn.addWidget(self.worker_label)
        root.addWidget(conn_group)

        mode_group = QGroupBox("Tool mode")
        mode_row = QHBoxLayout(mode_group)
        self.mode_button_group = QButtonGroup(self)
        self.mode_button_group.setExclusive(True)
        self.mode_buttons: dict[str, QPushButton] = {}
        for mode, label in (("agent", "Agent"), ("shell", "Shell"), ("both", "Both")):
            button = QPushButton(label)
            button.setCheckable(True)
            button.setProperty("mode", mode)
            self.mode_button_group.addButton(button)
            self.mode_buttons[mode] = button
            mode_row.addWidget(button)
            button.clicked.connect(lambda _checked=False, selected=mode: self._set_mode(selected))
        self.mode_buttons[self._mode].setChecked(True)
        root.addWidget(mode_group)

        actions = QGroupBox("Agent workflow")
        action_grid = QGridLayout(actions)
        self.health_button = self._new_button("Check bridge", "bridge_health")
        self.panes_button = self._new_button("List workers", "list_panes")
        self.delegate_button = self._new_button("Delegate task", "delegate_work", primary=True)
        self.new_task_button = QPushButton("New task")
        self.new_task_button.setToolTip("Clear retry and tracking context for a fresh delegation. Existing work keeps running.")
        self.get_button = self._new_button("Get operation status", "get_operation", connect=False)
        self.wait_button = self._new_button("Wait for update", "wait_for_operation", connect=False)
        self.cancel_op_button = self._new_button("Cancel operation", "cancel_operation", danger=True, connect=False)
        self.run_agent_button = self._new_button("Run agent", "run_agent", primary=True)
        self.list_agents_button = self._new_button("List agents", "list_agents")
        self.wait_agents_button = self._new_button("Wait for agents", "wait_agents")
        self.cancel_agent_button = self._new_button("Cancel agent", "cancel_agent", danger=True)
        self.transcript_button = self._new_button("Agent transcript", "agent_transcript")
        self.take_over_button = self._new_button("Take over worker", "take_over")
        self.cancel_request_button = QPushButton("Cancel wait")
        self.cancel_request_button.setObjectName("danger")
        self.cancel_request_button.setEnabled(False)
        buttons = [self.health_button, self.panes_button, self.delegate_button,
                   self.run_agent_button, self.list_agents_button, self.wait_agents_button,
                   self.cancel_agent_button, self.transcript_button, self.take_over_button,
                   self.new_task_button, self.get_button, self.wait_button,
                   self.cancel_op_button, self.cancel_request_button]
        for i, button in enumerate(buttons):
            action_grid.addWidget(button, i // 4, i % 4)
        root.addWidget(actions)

        tools_group = QGroupBox("All MCP tools")
        tools_layout = QVBoxLayout(tools_group)
        hint = QLabel("Each button opens a typed form when the tool needs arguments.")
        hint.setObjectName("note")
        tools_layout.addWidget(hint)
        self.tools_scroll = QScrollArea()
        self.tools_scroll.setWidgetResizable(True)
        self.tools_container = QWidget()
        self.tools_grid = QGridLayout(self.tools_container)
        self.tools_grid.setAlignment(Qt.AlignmentFlag.AlignTop)
        for column in range(3):
            self.tools_grid.setColumnStretch(column, 1)
        self.tools_scroll.setWidget(self.tools_container)
        self.tools_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.tools_scroll.setMinimumHeight(130)
        tools_layout.addWidget(self.tools_scroll)
        root.addWidget(tools_group)

        output_group = QGroupBox("Operation and tool output")
        output_layout = QVBoxLayout(output_group)
        status_row = QHBoxLayout()
        self.operation_status = QLabel("No operation selected")
        self.operation_status.setObjectName("opstatus")
        self.operation_stage = QLabel("")
        self.operation_stage.setObjectName("note")
        status_row.addWidget(self.operation_status)
        status_row.addWidget(self.operation_stage, 1)
        status_row.addStretch(1)
        output_layout.addLayout(status_row)
        self.result_view = QPlainTextEdit()
        self.result_view.setReadOnly(True)
        self.result_view.setPlaceholderText("Results and worker progress appear here.")
        output_layout.addWidget(self.result_view)
        root.addWidget(output_group, 1)
        self.setCentralWidget(central)

        self.connect_button.clicked.connect(self._connect)
        self.disconnect_button.clicked.connect(self.transport.disconnect)
        self.project_button.clicked.connect(self._choose_project)
        self.worker_button.clicked.connect(self._choose_worker)
        self.get_button.clicked.connect(self._get_operation)
        self.wait_button.clicked.connect(self._wait_operation)
        self.cancel_op_button.clicked.connect(self._cancel_operation)
        self.new_task_button.clicked.connect(self._new_task)
        self.cancel_request_button.clicked.connect(self._cancel_request)

    def _new_button(self, label: str, tool_name: str, primary: bool = False,
                    danger: bool = False, connect: bool = True) -> QPushButton:
        button = QPushButton(label)
        if primary:
            button.setObjectName("primary")
        elif danger:
            button.setObjectName("danger")
        if connect:
            button.clicked.connect(lambda _checked=False, name=tool_name: self._run_tool(name))
        return button

    def _connect_transport(self) -> None:
        self.transport.connected.connect(self._on_connected)
        self.transport.disconnected.connect(self._on_disconnected)
        self.transport.tools_received.connect(self._on_tools)
        self.transport.result_received.connect(self._on_result)
        self.transport.request_failed.connect(self._on_failure)
        self.transport.progress_received.connect(self._on_progress)
        self.transport.log_received.connect(self._log)

    def _connect(self) -> None:
        program, args = default_command()
        try:
            self.transport.start(program, args, str(REPO_ROOT))
            self.connect_button.setEnabled(False)
            self._log(f"Starting local MCP: {program}")
        except Exception as exc:
            QMessageBox.critical(self, "Could not connect", str(exc))

    def _on_connected(self) -> None:
        self._connected = True
        self.connection_status.setText("Connected · discovering tools")
        self.connect_button.setEnabled(False)
        self.disconnect_button.setEnabled(True)

    def _on_disconnected(self) -> None:
        self._connected = False
        self.connection_status.setText("Disconnected")
        self.connect_button.setEnabled(True)
        self.disconnect_button.setEnabled(False)
        self._log("MCP disconnected")

    def _on_tools(self, tools: list) -> None:
        self._tools = {item["name"]: item for item in tools
                       if isinstance(item, dict) and isinstance(item.get("name"), str)}
        names_available = set(self._tools)
        quick_buttons = {
            "bridge_health": self.health_button,
            "list_panes": self.panes_button,
            "delegate_work": self.delegate_button,
            "get_operation": self.get_button,
            "wait_for_operation": self.wait_button,
            "cancel_operation": self.cancel_op_button,
            "run_agent": self.run_agent_button,
            "list_agents": self.list_agents_button,
            "wait_agents": self.wait_agents_button,
            "cancel_agent": self.cancel_agent_button,
            "agent_transcript": self.transcript_button,
            "take_over": self.take_over_button,
        }
        self.tool_buttons = {name: button for name, button in quick_buttons.items() if name in names_available}
        for name, button in quick_buttons.items():
            button.setEnabled(name in names_available)
        if "run_agent" in names_available:
            self._set_mode_visual("both" if "shell_exec" in names_available else "agent")
        elif "shell_exec" in names_available:
            self._set_mode_visual("shell")
        while self.tools_grid.count():
            item = self.tools_grid.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        names = sorted(name for name in self._tools if name not in AGENT_CATALOG_TOOLS | set(quick_buttons) | {"set_mode"})
        for index, name in enumerate(names):
            tool = self._tools[name]
            label = BUTTON_LABELS.get(name, name.replace("_", " ").title())
            button = QPushButton(label)
            button.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
            button.setMinimumWidth(0)
            button.setToolTip(str(tool.get("description", "")))
            if name == "delegate_work":
                button.setObjectName("primary")
            elif name in {"cancel_operation", "kill_pane_process"}:
                button.setObjectName("danger")
            button.clicked.connect(lambda _checked=False, tool_name=name: self._run_tool(tool_name))
            self.tools_grid.addWidget(button, index // 3, index % 3)
            self.tool_buttons[name] = button
        for name, button in quick_buttons.items():
            if name in names_available:
                button.setToolTip(str(self._tools[name].get("description", "")))
        count = len(self._tools)
        self.connection_status.setText(f"Connected · {count} tools · {self._mode.title()} mode")
        self._log(f"Discovered {count} MCP tools")

    def _choose_project(self) -> None:
        path = QFileDialog.getExistingDirectory(self, "Choose project", self._project_path)
        if path:
            self._project_path = path
            self.project_label.setText(Path(path).name or path)
            self.project_label.setToolTip(path)
            self._pane_id = ""
            self._agent_type = "codex"
            self._agent_handle = ""
            self._agent_revision = 0
            self._agent_cache.clear()
            self._operation_id = ""
            self._revision = 0
            self.worker_label.setText("No worker selected")
            self.worker_label.setToolTip("")
            self.operation_status.setText("No operation selected")
            self.operation_stage.setText("")

    def _choose_worker(self) -> None:
        if not self._pane_cache:
            QMessageBox.information(self, "List workers first", "Use List workers to load available worker panes.")
            return
        pane = ask_worker_pane(self, self._pane_cache)
        if pane is None:
            return
        self._pane_id = _pane_id(pane)
        self._agent_type = str(pane.get("agent_type") or pane.get("agent") or "codex")
        self.worker_label.setText(_pane_label(pane))
        self.worker_label.setToolTip(self._pane_id)

    def _defaults_for_tool(self, name: str) -> dict[str, Any]:
        common: dict[str, Any] = {}
        schema = self._tools.get(name, {}).get("inputSchema", {})
        properties = schema.get("properties", {}) if isinstance(schema, dict) else {}
        if "project_path" in properties:
            common["project_path"] = self._project_path
        if "pane_id" in properties and self._pane_id:
            common["pane_id"] = self._pane_id
        if "operation_id" in properties and self._operation_id:
            common["operation_id"] = self._operation_id
        if "handle" in properties and self._agent_handle:
            common["handle"] = self._agent_handle
        if "handles" in properties and self._agent_handle:
            common["handles"] = [self._agent_handle]
        if "after_revisions" in properties and self._agent_handle:
            common["after_revisions"] = {self._agent_handle: self._agent_revision}
        if name == "delegate_work":
            common.setdefault("agent_type", self._agent_type)
            common.setdefault("acceptance_criteria", ["Complete the requested task and report the result."])
            if self._pane_id and "pane_id" in properties:
                common["pane_id"] = self._pane_id
        if name == "run_agent":
            available_agents = properties.get("agent_type", {}).get("enum", [])
            common.setdefault("agent_type", self._agent_type if self._agent_type in available_agents else "codex")
            common.setdefault("acceptance_criteria", ["Complete the requested task and report the result."])
        if name == "wait_for_operation":
            common.setdefault("after_revision", self._revision)
            common.setdefault("until", TERMINAL_STATES)
            common.setdefault("timeout_ms", 30_000)
        return common

    def _set_mode_visual(self, mode: str) -> None:
        if mode not in MODES:
            return
        self._mode = mode
        self.mode_buttons[mode].setChecked(True)
        if self._connected:
            count = len(self._tools)
            self.connection_status.setText(f"Connected · {count} tools · {mode.title()} mode")

    def _set_mode(self, mode: str) -> None:
        if mode not in MODES or mode == self._mode:
            return
        if "set_mode" not in self._tools:
            self.mode_buttons[self._mode].setChecked(True)
            QMessageBox.information(self, "Mode unavailable", "The MCP server did not advertise set_mode.")
            return
        if not self._connected:
            self.mode_buttons[self._mode].setChecked(True)
            QMessageBox.information(self, "Not connected", "Connect to the local MCP process first.")
            return
        self._requested_mode = mode
        self._call("set_mode", {"mode": mode})

    def _operation_context(self) -> dict[str, Any] | None:
        if not self._operation_id:
            return None
        context: dict[str, Any] = {
            "operation_id": self._operation_id,
            "project_path": self._project_path,
        }
        return context

    def _new_task(self) -> None:
        # A deliberate same-task rerun needs a fresh idempotency key; existing work
        # remains live and is never cancelled by this UI action.
        self._idempotency_keys.clear()
        self._operation_id = ""
        self._revision = 0
        self.operation_status.setText("Ready for a new task")
        self.operation_stage.setText("Existing work continues")
        self.result_view.clear()

    def _get_operation(self) -> None:
        args = self._operation_context()
        if args is None:
            self._run_tool("get_operation")
            return
        self._call("get_operation", args)

    def _cancel_operation(self) -> None:
        args = self._operation_context()
        if args is None:
            self._run_tool("cancel_operation")
            return
        self._call("cancel_operation", args)

    def _wait_operation(self) -> None:
        args = self._operation_context()
        if args is None:
            self._run_tool("wait_for_operation")
            return
        args.update({
            "after_revision": self._revision,
            "until": TERMINAL_STATES,
            "timeout_ms": 30_000,
        })
        self._call("wait_for_operation", args, timeout_ms=35_000)

    def _run_tool(self, name: str) -> None:
        if not self._connected:
            QMessageBox.information(self, "Not connected", "Connect to the local MCP process first.")
            return
        tool = self._tools.get(name)
        if tool is None:
            QMessageBox.information(self, "Tool unavailable", f"The MCP server did not advertise {name}.")
            return
        schema = tool.get("inputSchema", {})
        props = schema.get("properties", {}) if isinstance(schema, dict) else {}
        defaults = self._defaults_for_tool(name)
        form_tool = tool
        if name == "delegate_work" and "idempotency_key" in props:
            form_tool = dict(tool)
            form_tool["inputSchema"] = dict(schema)
            form_tool["inputSchema"]["properties"] = dict(props)
            form_tool["inputSchema"]["properties"].pop("idempotency_key", None)
            form_tool["inputSchema"]["required"] = [
                key for key in schema.get("required", []) if key != "idempotency_key"
            ]
        if props:
            args = ask_tool_arguments(self, form_tool, defaults)
            if args is None:
                return
        else:
            args = {}
        if name == "delegate_work":
            fingerprint = json.dumps(args, sort_keys=True, ensure_ascii=False)
            key = self._idempotency_keys.setdefault(fingerprint, str(uuid.uuid4()))
            args["idempotency_key"] = key
            self._log("Delegation uses a stable idempotency key for exact retries.")
        if name == "delegate_work" and self._pane_id and "pane_id" not in args:
            args["pane_id"] = self._pane_id
        self._call(name, args)

    def _call(self, name: str, args: dict[str, Any], timeout_ms: int | None = None) -> None:
        if not self._connected:
            QMessageBox.information(self, "Not connected", "Connect to the local MCP process first.")
            return
        if timeout_ms is None:
            wait_ms = args.get("timeout_ms", 120_000) if name == "wait_for_operation" else 30_000
            timeout_ms = int(wait_ms) + 5_000 if isinstance(wait_ms, int) and wait_ms > 0 else 35_000
        try:
            request_id = self.transport.call_tool(name, args, timeout_ms=timeout_ms)
        except Exception as exc:
            message = f"Could not call {name}: {exc}"
            self.result_view.setPlainText(message)
            self.operation_status.setText("Request could not be sent")
            self._log(message)
            return
        self._active_requests[request_id] = name
        self.cancel_request_button.setEnabled(any(tool == "wait_for_operation" for tool in self._active_requests.values()))
        self._log(f"→ {name}")
        if name == "wait_for_operation":
            self.operation_status.setText("Waiting for operation update…")

    def _cancel_request(self) -> None:
        waiting = [request_id for request_id, tool in self._active_requests.items()
                   if tool == "wait_for_operation"]
        if not waiting:
            return
        request_id = waiting[-1]
        self.transport.cancel_request(request_id)

    def _on_result(self, request_id: int, name: str, result: Any) -> None:
        self._finish_request(request_id)
        human = render_result(result)
        self.result_view.setPlainText(human)
        if isinstance(result, dict) and result.get("isError") is True:
            if name == "set_mode":
                self.mode_buttons[self._mode].setChecked(True)
            self.operation_status.setText(f"{name} returned an error")
            self._log(f"✕ {name}: {human}")
            return
        self._log(f"✓ {name}")
        if name == "list_panes":
            self._pane_cache = _extract_panes(result)
            self._log(f"Loaded {len(self._pane_cache)} worker panes")
        if name == "set_mode":
            mode = self._requested_mode
            structured = result.get("structuredContent") if isinstance(result, dict) else None
            if isinstance(structured, dict) and structured.get("mode") in MODES:
                mode = str(structured["mode"])
            self._set_mode_visual(mode)
            refresh = getattr(self.transport, "refresh_tools", None)
            if callable(refresh):
                refresh()
        runs = agent_runs(result)
        if runs:
            self._record_agents(runs, focus=name in {"run_agent", "send_agent"})
            selected = next((run for run in runs if run.get("handle") == self._agent_handle), runs[0])
            self._show_agent(selected)
        elif name == "agent_transcript" and self._agent_handle:
            self.operation_status.setText(f"Transcript · {self._agent_handle[:10]}")
            self.result_view.setPlainText(human)
        snap = operation_snapshot(result)
        if snap and not runs:
            self._show_operation(snap, force=name == "delegate_work")
        elif name == "wait_for_operation":
            self.operation_status.setText("Wait returned; check operation state")

    def _on_failure(self, request_id: int, name: str, message: str) -> None:
        self._finish_request(request_id)
        if name == "set_mode":
            self.mode_buttons[self._mode].setChecked(True)
        self.operation_status.setText(f"{name} failed")
        self.result_view.setPlainText(message)
        self._log(f"✕ {name}: {message}")

    def _finish_request(self, request_id: int) -> None:
        self._active_requests.pop(request_id, None)
        self.cancel_request_button.setEnabled(any(tool == "wait_for_operation" for tool in self._active_requests.values()))

    def _on_progress(self, event: Any) -> None:
        runs = agent_runs(event)
        if runs:
            self._record_agents(runs)
            selected = next((run for run in runs if run.get("handle") == self._agent_handle), runs[0])
            self._show_agent(selected)
        snap = operation_snapshot(event)
        if not snap:
            if not runs:
                self._log("Progress update received")
            return
        if self._operation_id and str(snap.get("operation_id")) != self._operation_id:
            return
        self._show_operation(snap)
        self._log(f"Progress · {snap.get('status', 'updated')} · revision {snap.get('revision', '?')}")

    def _record_agents(self, runs: list[dict[str, Any]], focus: bool = False) -> None:
        for run in runs:
            handle = run.get("handle")
            if not isinstance(handle, str) or not handle:
                continue
            try:
                revision = int(run.get("revision", 0))
            except (TypeError, ValueError):
                revision = 0
            previous = self._agent_cache.get(handle, {})
            try:
                previous_revision = int(previous.get("revision", 0))
            except (TypeError, ValueError):
                previous_revision = 0
            if revision >= previous_revision:
                self._agent_cache[handle] = dict(run)
            if focus or not self._agent_handle:
                self._agent_handle = handle
                self._agent_revision = revision

    def _show_agent(self, run: dict[str, Any]) -> None:
        handle = run.get("handle")
        if not isinstance(handle, str) or not handle:
            return
        self._agent_handle = handle
        try:
            self._agent_revision = int(run.get("revision", 0))
        except (TypeError, ValueError):
            self._agent_revision = 0
        status = str(run.get("status", "unknown"))
        self.operation_status.setText(f"Agent {status} · {handle[:10]}")
        self.operation_stage.setText(str(run.get("stage") or ""))
        body = render_result(run)
        prompt = run.get("prompt")
        if prompt:
            body += "\n\nAgent prompt requires input:\n" + render_result(prompt)
        self.result_view.setPlainText(body)

    def _show_operation(self, snapshot: dict[str, Any], force: bool = False) -> None:
        incoming_id = str(snapshot.get("operation_id", ""))
        if not incoming_id:
            return
        if self._operation_id and self._operation_id != incoming_id and not force:
            return
        try:
            incoming_revision = int(snapshot.get("revision", 0))
        except (TypeError, ValueError):
            incoming_revision = 0
        if self._operation_id == incoming_id and incoming_revision < self._revision:
            return
        self._operation_id = incoming_id
        self._revision = incoming_revision
        status = str(snapshot.get("status", "unknown"))
        self.operation_status.setText(f"{status} · revision {incoming_revision}")
        self.operation_stage.setText(str(snapshot.get("stage") or ""))
        body = render_result(snapshot)
        if snapshot.get("required_action"):
            body += "\n\nRequired action (resolve manually):\n" + render_result(snapshot["required_action"])
        self.result_view.setPlainText(body)

    def _log(self, message: str) -> None:
        stamp = datetime.now().strftime("%H:%M:%S")
        self.connection_status.setToolTip(f"[{stamp}] {message}")

    def _apply_style(self) -> None:
        self.setStyleSheet("""
            QWidget { background: #10141c; color: #e5eaf2; font-size: 13px; }
            QGroupBox { border: 1px solid #293344; border-radius: 9px; margin-top: 10px; padding: 12px 10px 10px; font-weight: 600; }
            QGroupBox::title { subcontrol-origin: margin; left: 12px; padding: 0 5px; color: #aab7c9; }
            QPushButton { background: #222d3c; border: 1px solid #35445a; border-radius: 6px; padding: 9px 12px; }
            QPushButton:hover { background: #2c3b50; }
            QPushButton:disabled { color: #657184; background: #181e27; }
            QPushButton#primary { background: #2362a5; border-color: #347cc7; font-weight: 600; }
            QPushButton#primary:hover { background: #2a73be; }
            QPushButton#danger { color: #ffb7b7; border-color: #70404a; }
            QLabel#title { font-size: 22px; font-weight: 700; }
            QLabel#subtitle, QLabel#note { color: #91a0b4; }
            QLabel#connection { color: #82d5a6; font-weight: 600; }
            QLabel#opstatus { color: #8fc4ff; font-weight: 600; }
            QPlainTextEdit { background: #0b0f15; border: 1px solid #293344; border-radius: 6px; padding: 9px; font-family: Consolas, 'Cascadia Code', monospace; font-size: 12px; }
            QScrollArea { border: 0; }
        """)

    def closeEvent(self, event: Any) -> None:  # noqa: N802
        # Stop only this MCP client process. Worker operations keep running.
        self.transport.disconnect()
        event.accept()


def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName("Puppet Master MCP Console")
    window = MainWindow()
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
