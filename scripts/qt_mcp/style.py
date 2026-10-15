"""Console window stylesheet."""

STYLESHEET = """
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
"""
