# Ported files: sandbox desktop setup

Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, Apache-2.0. Copyright 2026 Rakazo contributors.
Source folder: `infra/sandboxes/computer/` (and `infra/compose/restrict-computer-egress.sh`) in the Rakazo clone, read only.
License text: `packages/potato-sandbox/LICENSE-APACHE-2.0`.

This folder also holds the recipe data in `../recipes/` (see `../recipes/SCHEMA.md`). Its files are listed below too.

Line counts: "kept" = lines of their file that appear unchanged in ours (from `diff`). "removed" = lines of theirs not in ours.
Files written from our own PM spike scripts (`D:\pm-sandbox\spike`, not Rakazo) have no Rakazo header and say so in their first lines.

## Table

| our file | their file | what changed | lines kept/removed |
|---|---|---|---|
| `guest/desktop/pm-sbx-browser` | `infra/sandboxes/computer/rakazo-browser` (442) | Chrome .deb only (no Chromium snap, no Brave); one profile per bot (`PM_SBX_BOT`); Chrome's own sandbox stays on unless `PM_SBX_CHROME_NO_SANDBOX=1`; `--ozone-platform=x11`; debug port from `PM_SBX_CDP_PORT` (default 9222) on 127.0.0.1; URL hand-off is one HTTP call (no retry loop and no uncertain-create reconciliation); LD_PRELOAD / nss_wrapper dropped; policy from `/etc/opt/chrome/policies/managed`. Kept: flags list, stale-lock cleanup, crash-flag reset, live-browser reuse rule. | 24 / 418 (ours 93 new lines) |
| `guest/desktop/pm-sbx-start-desktop.sh` | `infra/sandboxes/computer/start.sh` (128); `fluxbox.init`, `fluxbox.menu`; spike `start-desktop.sh` and `vnc.sh` (spike, not Rakazo) | Runs as user pm (no PID-1 trap, no control loop, no portal daemons, no Docker). Xvfb `:99` 1440x900. Session (fluxbox or XFCE) from `/etc/pm-sbx/desktop.conf`. Fluxbox config written into the pm state folder (no HOME override); menu entries are our browser, xterm, pcmanfm, mousepad. x11vnc `-listen 127.0.0.1 -viewonly -rfbauth` (their `-nopw` dropped). websockify on 127.0.0.1. Ports from env. Start/stop/status by PID file with a cmdline check (never by name). Refuses to start unless `/run/pm-no-wslg.done` exists and WSLg is gone. Default browser set with `xdg-settings` (their `xdg-mime` loop dropped). | 11 / 117 (ours 222 new lines) |
| `guest/desktop/pm-sbx-browser.desktop` | `infra/sandboxes/computer/rakazo-browser.desktop` (9) | name, comment, exec path | 6 / 3 |
| `guest/desktop/pm-sbx-chrome-policy.json` | `infra/sandboxes/computer/browser-policy.json` (5) | Chrome's own policy names: no password manager, no autofill, no browser sign-in and no sync, no metrics, no background mode, downloads go to `/home/pm/Downloads` without asking. Their Chromium policy path is replaced by Chrome's. | 4 / 1 |
| `guest/desktop/pm-no-wslg.sh` | `infra/compose/restrict-computer-egress.sh` (388), CIDR lists only; WSLg part from our spike `pm-no-wslg.sh` | Their file is a Docker DOCKER-USER firewall; we reuse only the CIDR list (IPv4: 0/8, 10/8, 100.64/10, 127/8, 169.254/16, 172.16/12, 192.0.0/24, 192.168/16, 198.18/15, 224/4, 240/4; IPv6: fc00::/7, fe80::/10, ff00::/8). Rules go in the distro's OUTPUT chain. WSL DNS (10.255.255.254) and the WSL gateway are handled. A probe must fail closed. WSLg unmount, `/tmp/.X11-unix` reset and socket cleanup come from the spike. | 14 CIDR entries kept (of 388 lines); rest new |
| `guest/desktop/wsl.conf` | none (our spike `create.ps1` wsl.conf) | automount off, interop off, appendWindowsPath off, default user pm, systemd off, `[boot] command` runs the WSLg/egress guard | n/a |
| `guest/desktop/profile-pm.sh` | none (our spike `pm-env.sh`) | Login shells get the display/runtime variables and lose WSLg and Pulse. Only non-root shells create the runtime dir. | n/a |
| `guest/desktop/apply-recipe.py` | none (original) | Stdlib installer. Reads recipe JSON, validates it, plans steps, runs apt, downloads .debs (https only, sha256 when pinned), runs pre/post lines, keeps a per-step digest so reruns skip finished steps, refuses if user `pm` has sudo, prints `PROGRESS n/total` lines and a `RESULT` line. Dry run needs no root. | n/a |
| `recipes/base.json` | `infra/sandboxes/computer/Dockerfile` (package list) | Kept from Dockerfile: xvfb, x11vnc, novnc, websockify, xdotool, x11-utils, x11-xserver-utils, dbus-x11, imagemagick, xclip, fonts-noto-cjk, fonts-noto-color-emoji, xterm, python3, procps, ca-certificates, curl, unzip (18 of the 36 items). Also used in other recipes: git, jq, openssh-client (coding.json), fluxbox (light.json). Dropped: chromium (now Google Chrome .deb), fluxbox (moved to light.json), wmctrl, bash, locales, libnss-wrapper, fonts-liberation, fonts-dejavu-core, xdg-desktop-portal*, xdg-utils, xsel, poppler-utils (PDF viewer is evince), pandoc, python3-openpyxl, brave, uv, gh pin, libnss-wrapper symlink. Added: xauth, fonts-noto, pcmanfm, mousepad, evince, zip, iproute2, iptables, gnupg. Post lines install the PM guest files and the pm user (no sudo). | n/a (data) |
| `recipes/light.json` | `fluxbox.init`, `fluxbox.apps`, `fluxbox.menu`; `Dockerfile` fluxbox line | Ubuntu's fluxbox package. Config is generated by `pm-sbx-start-desktop.sh`. `fluxbox.apps` (window rules) dropped; `fullMaximization` covers it. | n/a (data) |
| `recipes/coding.json` | `Dockerfile` (git, jq, openssh-client, unzip) | git, openssh-client, jq from Dockerfile; gh from Ubuntu's archive (their pinned tarball with sha256 dropped); Node.js from NodeSource (see decision below); package manager and language choices added. No uv, no Docker, no Brave. | n/a (data) |
| `recipes/xfce.json` | none (our spike `setup-desktop.sh`) | XFCE parts that were tested in the spike. Thunar and mousepad left out (base has pcmanfm and mousepad). | n/a (data) |
| `recipes/daily.json` | none (PM) | LibreOffice writer, calc and impress; media player and image viewer as options. | n/a (data) |
| `recipes/SCHEMA.md` | none (PM) | Documents the recipe format and the installer contract. | n/a |

Not ported (left for other pieces or dropped):

- `user-env.sh` (nss_wrapper passwd/group for bind-mounted homes): not needed; the pm user is a real Linux user.
- `start.sh` PID-1 handling, `rakazo-computer-control` loop, xdg-desktop-portal start, `websockify --token-plugin`: our control service and the sealed-link proxy are separate pieces. Our websockify has no token plugin; the server proxy checks the capability.
- `rakazo-local-bin.sh`, `embed.html`, `clipboard-bridge.js`, `mobile-keyboard.js`, `control.py`, `rakazo-focus-or-launch`, `rakazo-page-browser`, `xcapture.c`: other pieces (control, viewer, page browser).
- `fluxbox.apps` (window rules): dropped (see light.json).

## Decisions

1. **Node.js from NodeSource, major 24** (not the official tarball). It is signed, updated by apt, needs no per-release checksum and no hand-managed PATH, and fits the apt-only installer. Node 24 is the Active LTS line as far as we know; not verified against nodejs.org on 2026-11-01. Change `node_24.x` in `recipes/coding.json` to move the line.
2. **Chrome sandbox stays on by default.** The Chrome .deb ships `chrome-sandbox` setuid root (seen in the spike's suid list), so the user namespace path is not needed. Their `--no-sandbox` is only a fallback behind `PM_SBX_CHROME_NO_SANDBOX=1`. This must be checked in the live distro. If Chrome refuses to start with "no usable sandbox", the fix is that env flag, which is a security trade-off for the lead to decide.
3. **View-only VNC.** Viewer input is refused. Take-over input goes through the control service (not in this piece), as in Rakazo.
4. **Egress guard fails closed.** If `iptables` is missing, or the probe reaches the Windows gateway on 445, 135 or 3389, `/run/pm-no-wslg.done` is not written and the desktop refuses to start. The probe only means something when the Windows host listens on those ports (RPC 135 normally does, so a connect there would be a real leak).
5. **Desktop user has no sudo.** `base.json` creates `pm` with no groups beyond its own. `apply-recipe.py` checks `sudo`, `admin` and `wheel` groups and any sudoers line that names `pm`, and stops with exit 4 if it finds one.
6. **One CDP port per running browser.** The launcher uses one debug port per running browser (`PM_SBX_CDP_PORT`). The caller picks a different port for each bot that runs at the same time.

## Open items (not verified here)

- NodeSource repo key: the fingerprint is not pinned. The key is fetched over https at install time.
- Google Chrome .deb: the URL is rolling, so `sha256` is null. The installer logs the real hash (`UNPINNED` line) for pinning later. Chrome also installs its own apt source, so Chrome updates come through apt.
- Unpinned npm globals (pnpm, yarn, bun) and Ubuntu package versions (gh, rustc, golang-go, dotnet-sdk-8.0).
- Whether `gh` is in Ubuntu 24.04 universe, and whether `dotnet-sdk-8.0` and `openjdk-21-jdk-headless` are there, was not checked against the archive.
- VNC authentication uses only the first 8 characters of the password (VNC protocol limit). The password is random, local and loopback only.
- `x11vnc -storepasswd` takes the plain password on its command line, so it is briefly visible in `ps` inside the distro. The distro is single-user.
- `wsl.conf` changes need `wsl --terminate <distro>` on the host (not `--shutdown`). The host-side holder process that keeps the distro alive without systemd is not in this piece.
- Nothing was run in a live distro. Checks were static and dry-run only (see report).
- `pm-sbx-chrome-policy.json` cannot carry the provenance header (JSON has no comments, and Chrome reports unknown policy keys as errors). Its provenance is this table row.
- The recipes carry their provenance in a `_header` or `_source` key, which the installer ignores.
