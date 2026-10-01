"""Schema-driven, button-friendly MCP argument dialogs and result formatting."""

from __future__ import annotations

import re
import json
from typing import Any

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMessageBox,
    QPlainTextEdit,
    QPushButton,
    QScrollArea,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)


HIDDEN_CONTEXT_FIELDS = {"project_path", "idempotency_key", "operation_id", "after_revision"}
MULTILINE_FIELDS = {"task", "intent", "reason", "evidence", "summary", "description", "output_regex"}


def humanize(value: str) -> str:
    return re.sub(r"\s+", " ", value.replace("_", " ").replace("-", " ")).strip().capitalize()


def _schema(tool: dict[str, Any]) -> dict[str, Any]:
    schema = tool.get("inputSchema", tool.get("input_schema", {}))
    return schema if isinstance(schema, dict) else {}


class ToolArgumentsDialog(QDialog):
    """Render an MCP JSON Schema object as native Qt controls."""

    def __init__(self, parent: QWidget | None, tool: dict[str, Any], defaults: dict[str, Any] | None = None):
        self.tool = tool
        self.defaults = defaults if isinstance(defaults, dict) else {}
        self.fields: dict[str, tuple[QWidget, dict[str, Any], bool]] = {}
        self.hidden: dict[str, Any] = {}
        super().__init__(parent)
        self.setWindowTitle(humanize(str(tool.get("name", "MCP tool"))))
        self.setMinimumWidth(460)
        self.setMinimumHeight(300)

        outer = QVBoxLayout(self)
        description = str(tool.get("description", "")).strip()
        if description:
            label = QLabel(description)
            label.setWordWrap(True)
            label.setObjectName("toolDescription")
            outer.addWidget(label)

        self.form = QFormLayout()
        self.form.setLabelAlignment(Qt.AlignmentFlag.AlignRight)
        self.form.setFieldGrowthPolicy(QFormLayout.FieldGrowthPolicy.ExpandingFieldsGrow)
        self._add_properties(self.form, _schema(tool), self.defaults, prefix="")
        content = QWidget()
        content.setLayout(self.form)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setWidget(content)
        outer.addWidget(scroll, 1)

        self.error_label = QLabel()
        self.error_label.setStyleSheet("color: #b42318")
        self.error_label.setWordWrap(True)
        outer.addWidget(self.error_label)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Cancel)
        self.submit_button = buttons.addButton("Run", QDialogButtonBox.ButtonRole.AcceptRole)
        self.submit_button.setDefault(True)
        self.submit_button.clicked.connect(self._submit)
        buttons.rejected.connect(self.reject)
        outer.addWidget(buttons)

    def _add_properties(self, layout: QFormLayout, schema: dict[str, Any], defaults: dict[str, Any], prefix: str) -> None:
        properties = schema.get("properties", {})
        required = set(schema.get("required", []))
        if not isinstance(properties, dict):
            return
        for name, spec in properties.items():
            if not isinstance(spec, dict):
                continue
            path = f"{prefix}.{name}" if prefix else str(name)
            is_required = name in required
            value = defaults.get(name, spec.get("default"))
            if not prefix and name in HIDDEN_CONTEXT_FIELDS and name in defaults:
                self.hidden[path] = defaults[name]
                continue
            kind = spec.get("type", "string")
            if kind == "object":
                group = QGroupBox(humanize(name) + (" *" if is_required else ""))
                nested = QFormLayout(group)
                nested.setFieldGrowthPolicy(QFormLayout.FieldGrowthPolicy.ExpandingFieldsGrow)
                nested_defaults = value if isinstance(value, dict) else {}
                self._add_properties(nested, spec, nested_defaults, path)
                layout.addRow(group)
                continue
            widget = self._make_widget(name, spec, value)
            self.fields[path] = (widget, spec, is_required)
            label = humanize(name) + (" *" if is_required else "")
            layout.addRow(label, widget)

    def _make_widget(self, name: str, spec: dict[str, Any], value: Any) -> QWidget:
        kind = spec.get("type", "string")
        enum = spec.get("enum")
        item_spec = spec.get("items", {})
        if kind == "array" and not enum and isinstance(item_spec, dict):
            enum = item_spec.get("enum")
        if enum:
            if kind == "array":
                items = QListWidget()
                items.setMaximumHeight(110)
                items.setSelectionMode(QListWidget.SelectionMode.NoSelection)
                selected = set(value if isinstance(value, list) else [])
                for option in enum:
                    entry = QListWidgetItem(str(option), items)
                    entry.setFlags(entry.flags() | Qt.ItemFlag.ItemIsUserCheckable)
                    entry.setCheckState(Qt.CheckState.Checked if option in selected else Qt.CheckState.Unchecked)
                return items
            combo = QComboBox()
            combo.addItem("Choose…", None)
            for option in enum:
                combo.addItem(str(option), option)
            if value is not None:
                index = combo.findData(value)
                if index >= 0:
                    combo.setCurrentIndex(index)
            combo.setProperty("requiredChoice", value is not None)
            return combo
        if kind == "boolean":
            check = QCheckBox()
            check.setChecked(bool(value) if value is not None else False)
            return check
        if kind in ("integer", "number"):
            spin: QSpinBox | QDoubleSpinBox
            if kind == "integer":
                spin = QSpinBox()
                spin.setRange(int(spec.get("minimum", -2147483647)), int(spec.get("maximum", 2147483647)))
            else:
                spin = QDoubleSpinBox()
                spin.setDecimals(5)
                spin.setRange(float(spec.get("minimum", -1e12)), float(spec.get("maximum", 1e12)))
            if value is not None:
                spin.setValue(value)
            elif "default" not in spec:
                spin.setSpecialValueText("Not set")
                spin.setValue(spin.minimum())
                spin.setProperty("unset", True)
            else:
                spin.setValue(spec["default"])
            spin.setProperty("optionalNumber", not ("default" in spec))
            spin.valueChanged.connect(lambda _v, item=spin: item.setProperty("unset", False))
            return spin
        if kind == "array":
            text = QPlainTextEdit()
            text.setPlaceholderText("One item per line")
            text.setMaximumHeight(90)
            if isinstance(value, list):
                text.setPlainText("\n".join(str(item) for item in value))
            return text
        if name in MULTILINE_FIELDS or int(spec.get("maxLength", 0)) > 120:
            text = QPlainTextEdit()
            text.setMaximumHeight(110)
            if value is not None:
                text.setPlainText(str(value))
            text.setPlaceholderText(str(spec.get("description", "")))
            return text
        edit = QLineEdit()
        if value is not None:
            edit.setText(str(value))
        edit.setPlaceholderText(str(spec.get("description", "")))
        if spec.get("format") == "password":
            edit.setEchoMode(QLineEdit.EchoMode.Password)
        return edit

    def _read_widget(self, widget: QWidget, spec: dict[str, Any], required: bool) -> Any:
        kind = spec.get("type", "string")
        if isinstance(widget, QComboBox):
            value = widget.currentData()
            if required and value is None:
                raise ValueError("Choose an option.")
            return value
        if isinstance(widget, QCheckBox):
            return widget.isChecked()
        if isinstance(widget, QListWidget):
            values = [widget.item(i).text() for i in range(widget.count())
                      if widget.item(i).checkState() == Qt.CheckState.Checked]
            if required and len(values) < int(spec.get("minItems", 1)):
                raise ValueError("Select at least one option.")
            return values
        if isinstance(widget, QSpinBox):
            if widget.property("unset") and required:
                raise ValueError("Enter a number.")
            return None if widget.property("unset") else widget.value()
        if isinstance(widget, QPlainTextEdit):
            text = widget.toPlainText().strip()
            if kind == "array":
                values = [line.strip() for line in text.splitlines() if line.strip()]
                minimum = int(spec.get("minItems", 1 if required else 0))
                if len(values) < minimum:
                    raise ValueError(f"Enter at least {minimum} item(s), one per line.")
                item_spec = spec.get("items", {})
                if isinstance(item_spec, dict):
                    enum = item_spec.get("enum")
                    if enum:
                        invalid = [item for item in values if item not in enum]
                        if invalid:
                            raise ValueError("Choose only allowed values: " + ", ".join(map(str, enum)))
                    min_length = int(item_spec.get("minLength", 0))
                    if any(len(item) < min_length for item in values):
                        raise ValueError(f"Each item must contain at least {min_length} character(s).")
                return values
            if required and len(text) < int(spec.get("minLength", 1)):
                raise ValueError("This field is required.")
            return text
        if isinstance(widget, QLineEdit):
            text = widget.text().strip()
            if required and len(text) < int(spec.get("minLength", 1)):
                raise ValueError("This field is required.")
            return text
        if isinstance(widget, QDoubleSpinBox):
            if widget.property("unset") and required:
                raise ValueError("Enter a number.")
            return None if widget.property("unset") else widget.value()
        return None

    def values(self) -> dict[str, Any]:
        result: dict[str, Any] = dict(self.hidden)
        for path, (widget, spec, required) in self.fields.items():
            value = self._read_widget(widget, spec, required)
            if value is None:
                continue
            if isinstance(value, str) and not value and not required:
                continue
            if isinstance(value, list) and not value and not required:
                continue
            cursor = result
            segments = path.split(".")
            for segment in segments[:-1]:
                cursor = cursor.setdefault(segment, {})
            cursor[segments[-1]] = value
        return result

    def _submit(self) -> None:
        try:
            self.result_arguments = self.values()
        except (TypeError, ValueError) as exc:
            self.error_label.setText(str(exc))
            return
        self.accept()


def ask_tool_arguments(parent: QWidget | None, tool: dict[str, Any], defaults: dict[str, Any] | None = None) -> dict[str, Any] | None:
    dialog = ToolArgumentsDialog(parent, tool, defaults)
    if dialog.exec() == QDialog.DialogCode.Accepted:
        return dialog.result_arguments
    return None


def _format_value(value: Any, label: str | None = None) -> list[str]:
    prefix = f"{humanize(label)}: " if label else ""
    if isinstance(value, dict):
        lines = [prefix.rstrip(": ")] if label else []
        for key, child in value.items():
            lines.extend(_format_value(child, str(key)))
        return lines
    if isinstance(value, (list, tuple)):
        if not value:
            return [prefix + "(none)" if label else "(none)"]
        lines = []
        for item in value:
            rendered = _format_value(item)
            lines.extend("• " + line for line in rendered)
        return ([prefix.rstrip()] if label else []) + lines
    if value is None:
        rendered = "Not available"
    elif isinstance(value, bool):
        rendered = "Yes" if value else "No"
    elif label and label.lower() in {"status", "stage", "reason", "source", "kind"}:
        rendered = humanize(str(value))
    else:
        rendered = str(value)
    return [prefix + rendered]


def render_result(value: Any) -> str:
    """Format MCP results as readable labels and text, never raw JSON."""
    if not isinstance(value, dict):
        return "\n".join(_format_value(value))
    lines: list[str] = []
    structured = value.get("structuredContent")
    if "isError" in value and value.get("isError"):
        lines.append("Tool error")
    if structured is not None:
        lines.extend(_format_value(structured))
    elif "content" in value:
        content = value.get("content", [])
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict):
                    if block.get("type") == "text":
                        text = str(block.get("text", ""))
                        try:
                            decoded = json.loads(text)
                        except (json.JSONDecodeError, TypeError):
                            lines.append(text)
                        else:
                            lines.extend(_format_value(decoded))
                    elif block.get("type") == "resource":
                        resource = block.get("resource", {})
                        if isinstance(resource, dict) and resource.get("text"):
                            lines.append(str(resource["text"]))
                    elif block.get("type") in ("image", "audio"):
                        lines.append(f"{humanize(str(block['type']))} result attached")
                    else:
                        lines.extend(_format_value(block))
                else:
                    lines.extend(_format_value(block))
    else:
        # Operation panes also pass snapshots directly instead of MCP envelopes.
        lines.extend(_format_value(value))
    if not lines and "error" in value:
        lines.extend(_format_value(value["error"], "Error"))
    return "\n".join(line for line in lines if line is not None).strip() or "No result content."
