# PORTED.md: guest control server

Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, Apache-2.0.
Source folder: `infra/sandboxes/computer/` in the Rakazo clone (read only).

| our file | their file | what changed | lines kept / removed (matched by difflib; source 399 / 162 lines) |
|---|---|---|---|
| `pm-sbx-control` (536 lines) | `infra/sandboxes/computer/control.py` (399) | See the list below. The xdotool allowlist (`_is_int_string`, `allowed_xdotool_argv`, `_allowed_wheel`, `ASCII_INT`, `ASCII_COUNT`) is byte-identical after stripping comments and indentation (checked with a script). | 134 kept / 265 removed (402 added) |
| `pm-sbx-focus-or-launch` (158 lines) | `infra/sandboxes/computer/rakazo-focus-or-launch` (162) | WM_CLASS table replaced by `--wm-class`; `RAKAZO_BROWSER_PROFILE` env and the `/tmp/rakazo/browser-profile-N` file replaced by `--profile DIR` (passed as `--user-data-dir=DIR` on launch); `LAUNCH_WAIT_SEC` replaced by `--wait`; argparse; launch OSError returns 127 instead of a traceback. Window matching, `/proc` profile reading, spawn-without-wait and raise-after-forward logic are unchanged. | 109 kept / 53 removed (49 added) |
| `test_control.py` (572 lines) | none (new) | New unit tests for the allowlist, argv building, config, auth, the focus wrapper and an HTTP round trip with subprocess and capture mocked. No X server needed. | n/a |

## pm-sbx-control changes

- Renamed: `rakazo-*` to `pm-sbx-*`, `RAKAZO_*` to `PM_SBX_*`, user `rakazo` to `pm` (profile path `/home/pm/.browser-profiles/chromium-bot-<32 hex>`).
- Auth: token read from the file named by `PM_SBX_CONTROL_TOKEN_FILE` at start (whitespace stripped, at least 16 characters, no spaces). Empty or missing token stops startup (exit 2). `Authorization` must be `Bearer <token>`; comparison is `hmac.compare_digest` on UTF-8 bytes, so a non-ASCII header is a 401 instead of an exception. Upstream accepted a header with no `Bearer ` prefix.
- Bind: `127.0.0.1` only, port from `PM_SBX_CONTROL_PORT` (default 47801). Upstream bound `0.0.0.0:7070`.
- Default display `:99` (`PM_SBX_DISPLAY` overrides). Upstream default `:1`.
- Launch allowlist: argv form `["env", "DISPLAY=:N", ("PM_SBX_BROWSER_PROFILE=<path>",) "pm-sbx-focus-or-launch", NAME, ARG?]`. NAME must be a key of the launcher table: built-ins `chrome` (browser profile allowed), `terminal` (xterm), `files` (pcmanfm), `editor` (mousepad), or the JSON file named by `PM_SBX_LAUNCHERS_FILE` (replaces the built-ins). ARG is at most one non-empty string that does not start with `-`. The launcher path comes only from the table, so a client never names an executable.
- Removed: the `xdg-open` launch form and the direct (non-focus) launch forms. Every launch now goes through `pm-sbx-focus-or-launch` and the launcher table.
- Native capture removed: no ctypes, no `librakazo-xcapture.so`, no native xinput path. Every xdotool action runs as its own process. Screenshots use `import -window root png:-` (ImageMagick). Width and height come from the PNG IHDR chunk. Frames never carry a `damage` field, so the Rust side must not rely on it.
- Validation: every step of a request is checked before any step runs, so a bad step runs nothing (upstream ran the earlier steps first). At most 64 steps. `waitMs` and `settleMs` must be JSON integers (not strings or booleans) and are clamped to 0..5000.
- Errors: 400 for malformed or disallowed requests, 500 for failed or timed-out actions (messages fixed, no tool stderr), 401 for auth, 404 for unknown paths.
- New endpoint: `POST /v1/screenshot` with `{"display"}` returns `{"observation": ...}`, under the same display lock.
- Child environment: `WAYLAND_DISPLAY` removed; `XDG_RUNTIME_DIR=/tmp/pm-runtime`, `GDK_BACKEND=x11`, `QT_QPA_PLATFORM=xcb`, `DISPLAY` set, per the sandbox rules.
- Subprocesses: argv lists only, no `shell=True`, stdin from DEVNULL, timeouts on every call (10 s actions, 13.4 s focus wrapper, 10 s capture helpers).
- Focus wrapper is started as `[sys.executable, "-I", <pm-sbx-focus-or-launch next to this file, or PM_SBX_FOCUS_OR_LAUNCH>, ...]`, so it needs no exec bit.
- Upstream `ThreadingHTTPServer` and per-display lock are kept. `DISPLAY` must match `:[0-9]+`.

## pm-sbx-focus-or-launch changes

See the table row above. Usage: `pm-sbx-focus-or-launch [--wm-class CLASS] [--profile DIR] [--wait SEC] -- LAUNCHER [ARGS...]`. `--wm-class` defaults to the lowercase launcher basename. `--profile` is matched against `--user-data-dir` in `/proc/<pid>/cmdline`, and added to the launch as `--user-data-dir=DIR` when it has to spawn.

## Runtime needs in the guest (not installed by this folder)

- `xdotool`, `wmctrl`, ImageMagick `import`, Python 3 (stdlib only), and the launcher packages named in the table (or the JSON file). Ubuntu 24.04 package paths are the defaults: `/usr/bin/google-chrome`, `/usr/bin/xterm`, `/usr/bin/pcmanfm`, `/usr/bin/mousepad`.
- `/home/pm/.browser-profiles/` must exist for the per-bot Chrome profiles.
- No X server was started for this port (LIFT rules). Only static checks and mocked unit tests were run.

## Not ported (on purpose)

- `xcapture.c`, `librakazo-xcapture.so` and every ctypes call (use the `import` fallback).
- `rakazo-browser` (per-display Chrome launcher), `rakazo-page-browser`, `start.sh`, `Dockerfile`, egress scripts: other pieces.
- `xdg-open` launch form (a generic URL/file opener is wider than the brief's launch list).

## Known upstream behaviour kept on purpose

- (Fixed in the integration pass.) `_is_int_string` used `str.isdigit`, which also accepts non-ASCII digit characters such as `"²"` that `int()` rejects. It now uses the `ASCII_INT` regex; `test_integers_are_ascii_digits_only` covers it. This is the one place the allowlist differs from upstream.

## Checks run

- `python -I -m py_compile` on all three Python files: OK.
- `python -I -m unittest discover -s <this folder> -p "test_*.py"`: 50 tests, OK (49 + the ASCII digit test).
- Allowlist functions diffed against upstream: identical.
