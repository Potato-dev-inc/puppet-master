#!/usr/bin/env python3
"""Launch the Qt6 Puppet Master MCP client."""

from pathlib import Path
import sys


def main() -> int:
    module_dir = Path(__file__).resolve().parent / "qt_mcp"
    sys.path.insert(0, str(module_dir))
    try:
        from PySide6.QtWidgets import QApplication
        from gui import MainWindow
    except ModuleNotFoundError as error:
        if error.name and error.name.startswith("PySide6"):
            print(
                "Qt6 is missing. Install the GUI dependencies with:\n"
                f'  "{sys.executable}" -m pip install -r "{module_dir / "requirements.txt"}"',
                file=sys.stderr,
            )
            return 1
        raise
    app = QApplication(sys.argv)
    app.setApplicationName("Puppet Master MCP Console")
    app.setOrganizationName("Puppet Master")
    window = MainWindow()
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
