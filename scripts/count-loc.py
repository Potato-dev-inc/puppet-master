#!/usr/bin/env python3
"""Count lines of real code in this repo.

Only source-code files are counted. Docs, configs, logs, lockfiles,
data, and generated/vendored output are ignored.
"""

from __future__ import annotations

import os
import sys
from collections import defaultdict
from pathlib import Path

SKIP_DIRS = {
    ".git",
    ".claude",
    ".orphan-check",
    ".puppet-master",
    "node_modules",
    "dist",
    "dev-dist",
    "target",
    "pwa-dist",
    "build",
    "coverage",
    "__pycache__",
    ".venv",
    "venv",
}

EXT_TO_LANG = {
    ".ts": "TypeScript",
    ".tsx": "TypeScript",
    ".mts": "TypeScript",
    ".cts": "TypeScript",
    ".js": "JavaScript",
    ".jsx": "JavaScript",
    ".mjs": "JavaScript",
    ".cjs": "JavaScript",
    ".rs": "Rust",
    ".py": "Python",
    ".go": "Go",
    ".java": "Java",
    ".c": "C",
    ".h": "C",
    ".cpp": "C++",
    ".cc": "C++",
    ".hpp": "C++",
    ".cs": "C#",
    ".rb": "Ruby",
    ".php": "PHP",
    ".swift": "Swift",
    ".kt": "Kotlin",
    ".kts": "Kotlin",
    ".scala": "Scala",
    ".sh": "Shell",
    ".bash": "Shell",
    ".zsh": "Shell",
    ".ps1": "PowerShell",
    ".lua": "Lua",
    ".dart": "Dart",
    ".vue": "Vue",
    ".svelte": "Svelte",
    ".sql": "SQL",
    ".css": "CSS",
    ".scss": "SCSS",
    ".sass": "Sass",
    ".less": "Less",
    ".html": "HTML",
    ".htm": "HTML",
}

GENERATED_SUFFIXES = (
    ".min.js",
    ".min.css",
    ".bundle.js",
    ".bundle.cjs",
    ".bundle.mjs",
    ".d.ts",
)

LANGS_ORDER = [
    "TypeScript",
    "JavaScript",
    "Rust",
    "Python",
    "Go",
    "Java",
    "C",
    "C++",
    "C#",
    "Ruby",
    "PHP",
    "Swift",
    "Kotlin",
    "Scala",
    "Dart",
    "Lua",
    "Shell",
    "PowerShell",
    "Vue",
    "Svelte",
    "SQL",
    "CSS",
    "SCSS",
    "Sass",
    "Less",
    "HTML",
]


def count_lines(path: Path) -> int:
    try:
        text = path.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return 0
    if not text:
        return 0
    lines = text.splitlines()
    return len(lines)


def is_generated(name: str) -> bool:
    lower = name.lower()
    if any(lower.endswith(suffix) for suffix in GENERATED_SUFFIXES):
        return True
    return "generated" in lower


def main() -> int:
    root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent

    totals: dict[str, dict[str, int]] = defaultdict(lambda: {"files": 0, "lines": 0})

    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for name in filenames:
            path = Path(dirpath) / name
            lang = EXT_TO_LANG.get(path.suffix.lower())
            if not lang or is_generated(name):
                continue
            totals[lang]["files"] += 1
            totals[lang]["lines"] += count_lines(path)

    grand_files = sum(v["files"] for v in totals.values())
    grand_lines = sum(v["lines"] for v in totals.values())

    print(f"Lines of real code in {root}\n")
    print(f"{'Language':<14}{'Files':>8}{'Lines':>12}")
    print("-" * 34)
    for lang in LANGS_ORDER:
        stats = totals.get(lang)
        if not stats or stats["files"] == 0:
            continue
        print(f"{lang:<14}{stats['files']:>8}{stats['lines']:>12}")
    print("-" * 34)
    print(f"{'Total':<14}{grand_files:>8}{grand_lines:>12}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
