"""Modal dialogs used by the console window."""

from __future__ import annotations

from typing import Any

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QDialogButtonBox,
    QLabel,
    QListWidget,
    QListWidgetItem,
    QVBoxLayout,
    QWidget,
)

try:
    from discovery import pane_id_of, pane_label
except ImportError:
    from .discovery import pane_id_of, pane_label


def ask_worker_pane(parent: QWidget | None, panes: list[dict[str, Any]]) -> dict[str, Any] | None:
    dialog = QDialog(parent)
    dialog.setWindowTitle("Choose worker")
    dialog.setMinimumWidth(520)
    dialog.setMinimumHeight(280)
    layout = QVBoxLayout(dialog)
    layout.addWidget(QLabel("Worker panes"))
    listing = QListWidget()
    for pane in panes:
        item = QListWidgetItem(pane_label(pane))
        item.setToolTip(pane_id_of(pane))
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
