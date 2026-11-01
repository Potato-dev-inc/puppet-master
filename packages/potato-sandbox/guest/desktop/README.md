# Sandbox desktop (guest/desktop)

Files that run inside the sandbox distro (a PM-owned WSL2 Ubuntu 24.04). The installer `apply-recipe.py` copies them to
their places from `recipes/base.json`. This page covers boot, start-up and who can do what on the screen.

## Boot: one entry, one verdict

`/etc/wsl.conf` can hold a single `[boot] command`, so it points to `/usr/local/sbin/pm-sbx-boot.sh` (root). That script runs, in order:

1. `pm-no-wslg.sh` - cuts WSLg (no `/mnt/wslg`, a plain `/tmp/.X11-unix`, no drive mounts).
2. `pm-sbx-egress.sh` - firewall walls: no Windows host, no LAN, internet stays.
3. `pm-sbx-egress-probe.sh` - proves the walls (dropped TCP to the gateway, internet answers).

All three pass: `/run/pm-sbx/boot.done` is written. Any failure: `/run/pm-sbx/boot.failed` with `reason=...`. Log: `/var/log/pm-sbx/boot.log`.
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
| 47801 | `pm-sbx-control` (screenshot, mouse, keyboard, launch; for the bot) | bearer token in `~/.pm-sbx/control.token` |

## View and control are two credentials

One x11vnc, two random 8-character passwords, both in `~/.pm-sbx/vnc/` (mode 600):

- `view.plain` - the **view** password. The screen is shown, and x11vnc discards every mouse and key event from that connection.
- `control.plain` - the **control** password. The same screen, and mouse and keyboard work. This is take-over.
- `passwd` - the file x11vnc reads: the control password, then a line `__BEGIN_VIEWONLY__`, then the view password.

Why this design: it is one process and one port, so there is nothing to keep in sync and nothing extra to expose. The
server, not the browser page, enforces the difference (a page that is changed to send input still gets dropped on the view password).
The passwords are different on purpose; the server tells them apart by the one that was typed.

How the host uses it: read `view.plain` through `wsl -d <distro> -u pm` to show the watch view; read `control.plain` only
when a person takes over (the lease: bot waits, person holds). Give the viewer page the password in the URL **fragment**
(`embed.html#vnc=...`, plus `?control=1` for take-over). The page strips it from the address bar at once. Delete both
files to rotate the passwords; the next start makes new ones. Restart x11vnc after rotating so it reads the new file.

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
