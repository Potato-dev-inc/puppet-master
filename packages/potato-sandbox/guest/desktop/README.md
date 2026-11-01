# Sandbox desktop (guest/desktop)

Files that run inside the sandbox distro (a PM-owned WSL2 Ubuntu 24.04). The installer `apply-recipe.py` copies them to
their places from `recipes/base.json`. This page covers boot, start-up and who can do what on the screen.

## Boot: one entry, one verdict

`/etc/wsl.conf` can hold a single `[boot] command`, so it points to `/usr/local/sbin/pm-sbx-boot.sh` (root). That script runs, in order:

1. `pm-no-wslg.sh` - cuts WSLg (no `/mnt/wslg`, a plain `/tmp/.X11-unix`, no drive mounts).
2. `pm-sbx-egress.sh` - firewall walls: no Windows host, no LAN, internet stays.
3. `pm-sbx-egress-probe.sh` - proves the walls (dropped TCP to the gateway, internet answers).

All three pass: `/run/pm-sbx/boot.done` is written. Any failure: `/run/pm-sbx/boot.failed` with `reason=...`. Log: `/var/log/pm-sbx/boot.log`.
These two files are the only verdict. The app waits for them (up to about 90 s) and does not run the walls or the probe itself;
`pm-no-wslg.sh` writes no flag of its own.
WSL runs the command in the background and the probe waits on dropped connections, so a full boot takes tens of seconds.

`pm-sbx-start-desktop` refuses to start (exit 3) unless `boot.done` exists, no WSLg mount is present, and
`/run/pm-sbx/egress-probe.status` says `state=passed`. It waits up to `PM_SBX_BOOT_WAIT` seconds (default 90) for the hook.

`wsl.conf` also sets the host name `pm-sandbox` and turns `generateHosts` off, because WSL's generated `/etc/hosts` would
contain a line with the Windows PC name. The recipe writes `/etc/hosts` itself.

## Start the desktop

As user `pm`: `pm-sbx-start-desktop [start|stop|status]`. It starts Xvfb `:99`, the session, x11vnc, websockify and the control
service. Everything listens on loopback only (x11vnc uses `-localhost`; the script fails if any listener on its ports is not
`127.0.0.1` or `[::1]`). State lives in `~/.pm-sbx/`.

| Port | What | Auth |
|---|---|---|
| 5900 | x11vnc (VNC) | view or control password |
| 6080 | websockify: the viewer page (`/usr/local/share/pm-sbx/viewer` if installed, else stock noVNC) and the websocket `/websockify` | same VNC passwords |
| 47801 | `pm-sbx-control` (screenshot, mouse, keyboard, launch; for the bot) | bearer token from `/run/pm-sbx/control.token` |

## Who owns the secrets: the app

The control token and the two VNC passwords are made by the app (Rust, on Windows) at every start, pushed over stdin as root into
`/run/pm-sbx/` (a memory file system; it is empty after every stop) and kept in the app's memory only. Each file is owned by `pm`
with mode `0400`, because `pm` runs the services:

| File | Holds |
|---|---|
| `/run/pm-sbx/control.token` | bearer token of the control service (64 hex characters) |
| `/run/pm-sbx/vnc-view.pass` | the **view** password (8 letters/digits) |
| `/run/pm-sbx/vnc-control.pass` | the **control** password (8 letters/digits, different from view) |

`pm-sbx-start-desktop` uses those files when they are present and never invents a secret the app gave. A pushed file that is
present but unusable (empty, not 8 letters/digits, equal passwords, not readable) stops the start with exit 3. Only when
**no** file was pushed (starting by hand, without the app) it makes its own random values under `~/.pm-sbx/` and logs a line
that starts with `FALLBACK`. The fallback is for manual testing only.

The control service is started by `pm-sbx-start-desktop` and by nothing else. `pm-sbx-start-desktop stop` stops Chrome (found by
its program path among `pm`'s processes), then the services, then the display.

## View and control are two credentials

One x11vnc, two 8-character passwords. x11vnc reads `~/.pm-sbx/vnc/passwd`, which the start script builds from the pushed files
(also kept as `view.plain` and `control.plain`, mode 600):

- the **view** password: the screen is shown, and x11vnc discards every mouse and key event from that connection.
- the **control** password: the same screen, and mouse and keyboard work. This is take-over.
- `passwd` - the file x11vnc reads: the control password, then a line `__BEGIN_VIEWONLY__`, then the view password.

Why this design: it is one process and one port, so there is nothing to keep in sync and nothing extra to expose. The
server, not the browser page, enforces the difference (a page that is changed to send input still gets dropped on the view password).
The passwords are different on purpose; the server tells them apart by the one that was typed.

How the host uses it: the app already holds both passwords, so it never reads them back out of the distro. It hands the view
password to the viewer page for the watch view, and the control password only when a person takes over (the lease: bot waits,
person holds). Give the viewer page the password in the URL **fragment** (`embed.html#vnc=...`, plus `?control=1` for take-over).
The page strips it from the address bar at once. A new start gets new passwords.

Not done here: a lease that stops the bot while a person holds control (that belongs to the app), and rate limiting of wrong
passwords (loopback only, 8 random characters).

## Files

| File | What |
|---|---|
| `wsl.conf` | automount, interop, Windows PATH off; host name; the one boot command |
| `pm-sbx-boot.sh` | the boot entry (root) |
| `pm-no-wslg.sh` | WSLg cut (root, run by the boot entry) |
| `pm-sbx-start-desktop.sh` | start, stop, status of the desktop services (user `pm`) |
| `pm-sbx-browser`, `pm-sbx-browser.desktop`, `pm-sbx-chrome-policy.json` | Chrome launcher (debug port 9221 + display number, 9320 on `:99`), menu entry, policy |
| `pm-sbx-launchers.json` | the apps `pm-sbx-control` may launch (installed as `/etc/pm-sbx/launchers.json`) |
| `profile-pm.sh` | login shell environment (no WSLg, no Pulse) |
| `apply-recipe.py` | the installer for `../../recipes/*.json` |
