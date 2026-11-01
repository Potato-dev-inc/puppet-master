#!/bin/bash
# Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, files infra/sandboxes/computer/start.sh (Xvfb, dbus session, fluxbox config and startup, default browser, view-only x11vnc, websockify), infra/sandboxes/computer/fluxbox.init and fluxbox.menu.
# Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
# see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: runs as user pm, not PID 1 (no signal trap, no
# Docker, no supervisor loops, no Rakazo control service, no xdg portal daemons); display :99 at 1440x900; session (fluxbox or XFCE)
# from /etc/pm-sbx/desktop.conf; x11vnc and websockify bind loopback only (x11vnc -localhost; the start fails if any listener on
# our ports is not loopback) and a VNC password is required; start, stop and status use PID files (never pkill by name); refuses to
# start unless the boot hook finished (/run/pm-sbx/boot.done), WSLg is cut and the egress probe passed; fluxbox config is written
# into the pm state folder, not via a HOME override; menu entries use pm-sbx-browser; two separate VNC passwords (view and control)
# on one x11vnc instead of Rakazo's view-only server.
#
# Usage, as user pm inside the distro:   pm-sbx-start-desktop [start|stop|status]
# Optional env: PM_SBX_DISPLAY (:99), PM_SBX_GEOMETRY (1440x900), PM_SBX_VNC_PORT (5900), PM_SBX_NOVNC_PORT (6080),
#               PM_SBX_CONTROL_PORT (47801; the control service starts only when /usr/local/bin/pm-sbx-control is installed),
#               PM_SBX_STATE (~/.pm-sbx). The session is read from /etc/pm-sbx/desktop.conf (fluxbox by default).
#               PM_SBX_WEB_DIR (folder websockify serves; default /usr/local/share/pm-sbx/viewer when it holds embed.html,
#               else /usr/share/novnc), PM_SBX_BOOT_WAIT (seconds to wait for the boot hook, 90).
# Writes: $STATE/env (source it to join the session from outside), $STATE/pids/*.pid, $STATE/logs/*.log,
#         $STATE/vnc/{view.plain,control.plain,passwd} (x11vnc's files, built from the app's passwords).
#
# WHO OWNS THE SECRETS: the app (Puppet Master, on Windows). It generates the control token and the two VNC passwords per start and
# pushes them over stdin, as root, into /run/pm-sbx/ (a memory file system, emptied at every stop) BEFORE it calls this script:
#   /run/pm-sbx/control.token       bearer token of the control service
#   /run/pm-sbx/vnc-view.pass       the VIEW password (8 letters/digits)
#   /run/pm-sbx/vnc-control.pass    the CONTROL password (8 letters/digits, different)
# each owned by pm, mode 0400. This script uses those files when they are present and never invents a secret the app gave.
# The self-generated fallback (random values under $STATE) stays only for manual testing without the app, and it logs
# "FALLBACK" when it is used. A pushed file that is present but unusable stops the start (it never silently falls back).
#
# Two local VNC passwords, one x11vnc. The VIEW password gives a screen that cannot send input (x11vnc drops it
# server side). The CONTROL password gives mouse and keyboard (take-over). Bot actions do not use VNC; they go through
# the control service. The service is started here and nowhere else.
# stop also stops Chrome (everything the browser launcher started for this user), then the services, then the display.

set -u

ACTION="${1:-start}"
case "$ACTION" in start|stop|status) ;; *) echo "usage: $0 [start|stop|status]" >&2; exit 2 ;; esac

DISPLAY_NAME="${PM_SBX_DISPLAY:-:99}"
case "$DISPLAY_NAME" in :[0-9]|:[0-9][0-9]) ;; *) echo "bad PM_SBX_DISPLAY" >&2; exit 2 ;; esac
DNUM="${DISPLAY_NAME#:}"
GEOM="${PM_SBX_GEOMETRY:-1440x900}"
if ! printf '%s\n' "$GEOM" | grep -Eq '^[0-9]{3,5}x[0-9]{3,5}$'; then echo "bad PM_SBX_GEOMETRY" >&2; exit 2; fi
VNC_PORT="${PM_SBX_VNC_PORT:-5900}"
NOVNC_PORT="${PM_SBX_NOVNC_PORT:-6080}"
CONTROL_PORT="${PM_SBX_CONTROL_PORT:-47801}"
BOOT_WAIT="${PM_SBX_BOOT_WAIT:-90}"
case "$BOOT_WAIT" in ''|*[!0-9]*) echo "bad PM_SBX_BOOT_WAIT" >&2; exit 2 ;; esac
for p in "$VNC_PORT" "$NOVNC_PORT" "$CONTROL_PORT"; do
  if ! printf '%s\n' "$p" | grep -Eq '^[0-9]{4,5}$' || [ "$p" -lt 1024 ] || [ "$p" -gt 65535 ]; then
    echo "bad port $p (use 1024-65535)" >&2; exit 2
  fi
done
[ "$VNC_PORT" != "$NOVNC_PORT" ] && [ "$VNC_PORT" != "$CONTROL_PORT" ] && [ "$NOVNC_PORT" != "$CONTROL_PORT" ] || { echo "VNC, noVNC and control ports must differ" >&2; exit 2; }

STATE="${PM_SBX_STATE:-$HOME/.pm-sbx}"
PIDS="$STATE/pids"
LOGS="$STATE/logs"
VNCDIR="$STATE/vnc"
FBDIR="$STATE/fluxbox"
export XDG_RUNTIME_DIR=/tmp/pm-runtime
mkdir -p "$LOGS" 2>/dev/null

log() { echo "$(date -Is) $*" >>"$LOGS/start.log" 2>/dev/null; echo "$*"; }
die() { log "ERROR $1"; exit "${2:-1}"; }

# Only the session recipe decides which window manager runs. Every action reads it, so stop and status
# find the right process. Default is fluxbox.
SESSION=$(sed -n 's/^PM_SBX_SESSION=//p' /etc/pm-sbx/desktop.conf 2>/dev/null | head -1)
SESSION="${SESSION:-fluxbox}"
if [ "$ACTION" = start ]; then
  case "$SESSION" in fluxbox|xfce4) ;; *) die "unknown session '$SESSION' in /etc/pm-sbx/desktop.conf" 2 ;; esac
fi
session_bin() { case "$SESSION" in xfce4) echo xfce4-session ;; *) echo fluxbox ;; esac; }

# Join-the-session environment. Everything that touches the display sets these explicitly too.
unset WAYLAND_DISPLAY PULSE_SERVER XDG_SESSION_TYPE
export DISPLAY="$DISPLAY_NAME" GDK_BACKEND=x11 QT_QPA_PLATFORM=xcb SDL_VIDEODRIVER=x11

pidfile() { echo "$PIDS/$1.pid"; }
pid_of() { [ -r "$(pidfile "$1")" ] && cat "$(pidfile "$1")"; return 0; }
# A PID counts as ours only if it is alive AND its command line still names the program (PIDs get reused).
alive_named() { [ -n "$1" ] && kill -0 "$1" 2>/dev/null && tr '\0' ' ' <"/proc/$1/cmdline" 2>/dev/null | grep -Fq -- "$2"; }
name_pattern() {
  case "$1" in
    xvfb) echo Xvfb ;;
    session) session_bin ;;
    x11vnc) echo x11vnc ;;
    websockify) echo websockify ;;
    control) echo pm-sbx-control ;;
  esac
}
alive_key() { alive_named "$(pid_of "$1")" "$(name_pattern "$1")"; }
port_listening() { ss -ltn 2>/dev/null | awk '{print $4}' | grep -Fqx "127.0.0.1:$1"; }
wait_port() {
  for _ in $(seq 1 50); do port_listening "$1" && return 0; sleep 0.1; done
  die "port $1 ($2) did not open; see $LOGS/$2.log" 1
}

# Refuse to touch the display unless the one boot hook (pm-sbx-boot.sh) finished clean: WSLg cut, egress walls applied, probe passed.
# The hook is asynchronous (it takes tens of seconds because the probe waits for dropped connections), so wait for it.
BOOT_DONE=/run/pm-sbx/boot.done
BOOT_FAILED=/run/pm-sbx/boot.failed
PROBE_STATUS=/run/pm-sbx/egress-probe.status
# Pushed by the app (see the header).
PUSHED_TOKEN=/run/pm-sbx/control.token
PUSHED_VIEW=/run/pm-sbx/vnc-view.pass
PUSHED_CONTROL=/run/pm-sbx/vnc-control.pass
gate() {
  i=0
  while [ ! -f "$BOOT_DONE" ] && [ ! -f "$BOOT_FAILED" ] && [ "$i" -lt $((BOOT_WAIT * 10)) ]; do sleep 0.1; i=$((i + 1)); done
  [ -f "$BOOT_FAILED" ] && die "REFUSING: the boot hook failed ($(sed -n 's/^reason=//p' "$BOOT_FAILED" 2>/dev/null | head -1)); see /var/log/pm-sbx/boot.log" 3
  [ -f "$BOOT_DONE" ] || die "REFUSING: the boot hook has not finished (no $BOOT_DONE)" 3
  if grep -q ' /mnt/wslg' /proc/mounts || [ -e /mnt/wslg/runtime-dir/wayland-0 ] || [ -e /tmp/.X11-unix/X0 ]; then
    die "REFUSING: WSLg is reachable from this distro" 3
  fi
  grep -qx 'state=passed' "$PROBE_STATUS" 2>/dev/null || die "REFUSING: the egress probe has not passed (see $PROBE_STATUS)" 3
}

stop_one() {
  p=$(pid_of "$1")
  if alive_named "$p" "$2"; then kill "$p" 2>/dev/null && log "stopped $1 ($p)"; fi
  rm -f "$(pidfile "$1")"
}

# Chrome is started by pm-sbx-browser / pm-sbx-control with no PID file, so it is found by its exact program path and
# only among this user's processes (a stopped desktop must not leave a browser behind). TERM first, KILL after 5 s.
chrome_pids() { pgrep -u "$(id -u)" -f '^/opt/google/chrome/chrome( |$)' 2>/dev/null; }
stop_chrome() {
  pids=$(chrome_pids)
  [ -n "$pids" ] || return 0
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null
  for _ in $(seq 1 50); do [ -z "$(chrome_pids)" ] && break; sleep 0.1; done
  left=$(chrome_pids)
  # shellcheck disable=SC2086
  [ -z "$left" ] || kill -9 $left 2>/dev/null
  log "stopped Chrome"
}

start_xvfb() {
  if alive_key xvfb; then log "Xvfb already running on $DISPLAY_NAME"; return 0; fi
  rm -f "/tmp/.X${DNUM}-lock" "/tmp/.X11-unix/X${DNUM}" 2>/dev/null
  # A distro that was terminated with the desktop up (a crash, a cut-off set up) leaves the session's ICE sockets in /tmp, and the
  # next xfce4-session often gets the same process number ("server already running"). Only ours (this user's) can be removed here.
  # Nothing of ours runs at this point: Xvfb is not alive, so no session is.
  rm -f /tmp/.ICE-unix/* 2>/dev/null
  setsid nohup Xvfb "$DISPLAY_NAME" -screen 0 "${GEOM}x24" -nolisten tcp -ac >"$LOGS/xvfb.log" 2>&1 </dev/null &
  echo $! >"$(pidfile xvfb)"
  for _ in $(seq 1 50); do xdpyinfo -display "$DISPLAY_NAME" >/dev/null 2>&1 && break; sleep 0.1; done
  xdpyinfo -display "$DISPLAY_NAME" >/dev/null 2>&1 || die "Xvfb did not start; see $LOGS/xvfb.log" 1
  log "Xvfb up on $DISPLAY_NAME ($GEOM)"
}

write_fluxbox_config() {
  mkdir -p "$FBDIR"
  cat >"$FBDIR/menu" <<'EOF'
[begin] (PM Sandbox)
  [exec] (Browser) {/usr/local/bin/pm-sbx-browser}
  [exec] (Terminal) {xterm -bg rgb:11/11/13 -fg rgb:e8/e8/ea -cr rgb:e8/e8/ea -title Terminal -xrm 'XTerm*selectToClipboard: true'}
  [exec] (Files) {pcmanfm}
  [exec] (Editor) {mousepad}
[end]
EOF
  cat >"$FBDIR/init" <<EOF
session.menuFile: $FBDIR/menu
session.styleOverlay: $FBDIR/overlay
session.screen0.rootCommand: xsetroot -solid #111113
session.screen0.toolbar.visible: true
session.screen0.toolbar.height: 28
session.screen0.toolbar.placement: BottomCenter
session.screen0.toolbar.widthPercent: 100
session.screen0.workspaces: 1
session.screen0.fullMaximization: true
session.screen0.window.focus.alpha: 255
session.overlay.menuDelay: 0
EOF
  # The style says "set this wallpaper" at every start. Fluxbox does that by running `fbsetbg`, and the stock script pops up
  # "Something went wrong while setting the wallpaper" when no wallpaper tool is installed (the sandbox has none). Two fixes:
  # the overlay turns the style's background off and keeps a plain colour, and a small `fbsetbg` that only sets that colour
  # (and never fails) goes first on fluxbox's PATH, in case anything still asks for it.
  cat >"$FBDIR/overlay" <<'EOF'
background: none
rootCommand: xsetroot -solid #111113
EOF
  mkdir -p "$FBDIR/bin"
  cat >"$FBDIR/bin/fbsetbg" <<'EOF'
#!/bin/sh
# Stand-in for fluxbox's fbsetbg: a plain dark background, no pop-up, always exit 0.
xsetroot -solid '#111113' >/dev/null 2>&1
exit 0
EOF
  chmod 0755 "$FBDIR/bin/fbsetbg"
}

start_session() {
  BIN=$(session_bin)
  if alive_key session; then
    log "$BIN already running"
  else
    if [ "$SESSION" = fluxbox ]; then
      write_fluxbox_config
      xsetroot -solid "#111113" >/dev/null 2>&1 || true
      setsid nohup env PATH="$FBDIR/bin:$PATH" dbus-launch --exit-with-session fluxbox -rc "$FBDIR/init" >"$LOGS/session.log" 2>&1 </dev/null &
    else
      setsid nohup dbus-launch --exit-with-session xfce4-session >"$LOGS/session.log" 2>&1 </dev/null &
    fi
    p=""
    for _ in $(seq 1 100); do
      p=$(pgrep -u "$(id -un)" -x "$BIN" | head -1)
      [ -n "$p" ] && break
      sleep 0.1
    done
    [ -n "$p" ] || die "$BIN did not start; see $LOGS/session.log" 1
    echo "$p" >"$(pidfile session)"
    log "$BIN up (pid $p)"
  fi
  p=$(pid_of session)
  DBUS=$(tr '\0' '\n' <"/proc/$p/environ" 2>/dev/null | sed -n 's/^DBUS_SESSION_BUS_ADDRESS=//p' | head -1)
  if [ "$SESSION" = xfce4 ]; then
    for _ in $(seq 1 200); do pgrep -u "$(id -un)" -x xfwm4 >/dev/null && break; sleep 0.1; done
  fi
  if command -v xdg-settings >/dev/null 2>&1; then
    xdg-settings set default-web-browser pm-sbx-browser.desktop >/dev/null 2>&1 || log "warning: default browser not set"
  fi
}

rand8() { head -c 512 /dev/urandom | tr -dc 'A-Za-z0-9' | head -c 8; }

# Any listener on this port that is not loopback (for example *:5900 or [::]:5900) is a failure.
wide_listener() {
  ss -ltnH 2>/dev/null | awk '{print $4}' | grep -E ":$1\$" | grep -Fvx -e "127.0.0.1:$1" -e "[::1]:$1" | head -1
}
require_loopback() {
  w=$(wide_listener "$1")
  [ -z "$w" ] || die "REFUSING: $2 listens on $w, not loopback only" 3
}

# VNC authentication uses only the first 8 characters of a password. x11vnc reads two sections from the password file:
# the lines before __BEGIN_VIEWONLY__ give full access (control), the lines after it give a screen that cannot send input.
# The two passwords must differ, or the server could not tell which one was typed.
VNC_CHANGED=0
VNC_SOURCE=""
valid_vnc_password() { printf '%s\n' "$1" | grep -Eq '^[A-Za-z0-9]{8}$'; }
make_vnc_passwords() {
  VNC_CHANGED=0
  if [ -e "$PUSHED_VIEW" ] || [ -e "$PUSHED_CONTROL" ]; then
    [ -r "$PUSHED_VIEW" ] && [ -r "$PUSHED_CONTROL" ] || die "REFUSING: the app's VNC passwords are not both readable by $(id -un) in /run/pm-sbx" 3
    ctl=$(cat "$PUSHED_CONTROL"); view=$(cat "$PUSHED_VIEW")
    if ! valid_vnc_password "$ctl" || ! valid_vnc_password "$view" || [ "$ctl" = "$view" ]; then
      die "REFUSING: the app's VNC passwords must be two different 8-character letter/digit values" 3
    fi
    VNC_SOURCE="from the app"
  else
    VNC_SOURCE="self-generated fallback"
    if [ -s "$VNCDIR/view.plain" ] && [ -s "$VNCDIR/control.plain" ] && [ -s "$VNCDIR/passwd" ]; then
      log "FALLBACK: reusing self-generated VNC passwords (the app pushed none; manual testing only)"
      return 0
    fi
    log "FALLBACK: generating VNC passwords here (the app pushed none; manual testing only)"
    ctl=$(rand8)
    view=$(rand8)
    while [ "$view" = "$ctl" ] || [ "${#view}" -ne 8 ] || [ "${#ctl}" -ne 8 ]; do ctl=$(rand8); view=$(rand8); done
  fi
  want=$(printf '%s\n__BEGIN_VIEWONLY__\n%s' "$ctl" "$view")
  have=$(cat "$VNCDIR/passwd" 2>/dev/null || true)
  [ "$want" = "$have" ] && return 0
  VNC_CHANGED=1
  umask 077
  printf '%s' "$ctl" >"$VNCDIR/control.plain"
  printf '%s' "$view" >"$VNCDIR/view.plain"
  printf '%s\n' "$want" >"$VNCDIR/passwd"
  chmod 600 "$VNCDIR/control.plain" "$VNCDIR/view.plain" "$VNCDIR/passwd"
}

# websockify serves the viewer folder when the app installed it, else the stock noVNC pages.
web_dir() {
  if [ -n "${PM_SBX_WEB_DIR:-}" ]; then echo "$PM_SBX_WEB_DIR"
  elif [ -f /usr/local/share/pm-sbx/viewer/embed.html ]; then echo /usr/local/share/pm-sbx/viewer
  else echo /usr/share/novnc; fi
}

start_vnc() {
  mkdir -p "$VNCDIR" && chmod 700 "$VNCDIR"
  make_vnc_passwords
  # New passwords: a running x11vnc still holds the old ones.
  if [ "$VNC_CHANGED" = 1 ]; then stop_one x11vnc x11vnc; fi
  if ! alive_key x11vnc; then
    # -localhost binds loopback only. (-listen 127.0.0.1 still opened [::]:5900 on the spike.) No -viewonly: the password
    # typed decides view or control (see make_vnc_passwords).
    setsid nohup x11vnc -display "$DISPLAY_NAME" -localhost -rfbport "$VNC_PORT" -passwdfile "$VNCDIR/passwd" \
      -forever -shared -noxdamage -quiet -o "$LOGS/x11vnc.log" >/dev/null 2>&1 </dev/null &
    echo $! >"$(pidfile x11vnc)"
  fi
  wait_port "$VNC_PORT" x11vnc
  require_loopback "$VNC_PORT" x11vnc
  if ! alive_key websockify; then
    setsid nohup websockify --web "$(web_dir)" "127.0.0.1:$NOVNC_PORT" "127.0.0.1:$VNC_PORT"       >"$LOGS/websockify.log" 2>&1 </dev/null &
    echo $! >"$(pidfile websockify)"
  fi
  wait_port "$NOVNC_PORT" websockify
  require_loopback "$NOVNC_PORT" websockify
  log "VNC on 127.0.0.1:$VNC_PORT, noVNC on 127.0.0.1:$NOVNC_PORT (passwords: $VNC_SOURCE)"
}

# The control service (screenshots, mouse, keyboard, app launch for the bot) on 127.0.0.1 with a bearer token read from a file.
# This is the only place that starts it. The token is the one the app pushed; the fallback is for manual testing only.
hash_of() { sha256sum "$1" 2>/dev/null | cut -d' ' -f1; }
start_control() {
  [ -x /usr/local/bin/pm-sbx-control ] || { log "control service not installed; skipping"; return 0; }
  if [ -e "$PUSHED_TOKEN" ]; then
    { [ -s "$PUSHED_TOKEN" ] && [ -r "$PUSHED_TOKEN" ]; } || die "REFUSING: the app's control token in /run/pm-sbx is empty or not readable by $(id -un)" 3
    TOKEN_FILE="$PUSHED_TOKEN"
    TOKEN_SOURCE="from the app"
  else
    log "FALLBACK: generating the control token here (the app pushed none; manual testing only)"
    if [ ! -s "$STATE/control.token" ]; then
      (umask 077; head -c 512 /dev/urandom | tr -dc 'a-f0-9' | head -c 32 >"$STATE/control.token")
      chmod 600 "$STATE/control.token"
    fi
    TOKEN_FILE="$STATE/control.token"
    TOKEN_SOURCE="self-generated fallback"
  fi
  # A service that runs with another token (a second start with new secrets) is replaced.
  tokhash="$PIDS/control.tokenhash"
  if alive_key control && [ "$(hash_of "$TOKEN_FILE")" != "$(cat "$tokhash" 2>/dev/null)" ]; then stop_one control pm-sbx-control; fi
  if ! alive_key control; then
    LAUNCHERS=""
    [ -f /etc/pm-sbx/launchers.json ] && LAUNCHERS=/etc/pm-sbx/launchers.json
    setsid nohup env PM_SBX_CONTROL_TOKEN_FILE="$TOKEN_FILE" PM_SBX_CONTROL_PORT="$CONTROL_PORT" \
      PM_SBX_DISPLAY="$DISPLAY_NAME" ${LAUNCHERS:+PM_SBX_LAUNCHERS_FILE="$LAUNCHERS"} \
      ${DBUS:+DBUS_SESSION_BUS_ADDRESS="$DBUS"} \
      python3 -I /usr/local/bin/pm-sbx-control >"$LOGS/control.log" 2>&1 </dev/null &
    echo $! >"$(pidfile control)"
    hash_of "$TOKEN_FILE" >"$tokhash"
  fi
  wait_port "$CONTROL_PORT" control
  require_loopback "$CONTROL_PORT" control
  log "control service on 127.0.0.1:$CONTROL_PORT (token: $TOKEN_SOURCE)"
}

write_env() {
  mkdir -p "$STATE"
  {
    echo "unset WAYLAND_DISPLAY PULSE_SERVER XDG_SESSION_TYPE"
    echo "export DISPLAY=$DISPLAY_NAME XDG_RUNTIME_DIR=/tmp/pm-runtime GDK_BACKEND=x11 QT_QPA_PLATFORM=xcb SDL_VIDEODRIVER=x11"
    if printf '%s\n' "$DBUS" | grep -Eq '^unix:[A-Za-z0-9_=,/.:+-]+$'; then
      echo "export DBUS_SESSION_BUS_ADDRESS='$DBUS'"
    fi
  } >"$STATE/env"
  chmod 600 "$STATE/env"
}

do_start() {
  gate
  mkdir -p "$PIDS" "$LOGS" "$VNCDIR" "$FBDIR" && chmod 700 "$STATE"
  mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
  T0=$(date +%s)
  DBUS=""
  start_xvfb
  start_session
  start_vnc
  start_control
  write_env
  echo "DESKTOP_READY session=$SESSION display=$DISPLAY_NAME seconds=$(( $(date +%s) - T0 ))"
}

do_stop() {
  stop_chrome
  stop_one control pm-sbx-control
  stop_one websockify websockify
  stop_one x11vnc x11vnc
  stop_one session "$(session_bin)"
  stop_one xvfb Xvfb
  echo "DESKTOP_STOPPED"
}

do_status() {
  for key in xvfb session x11vnc websockify control; do
    if alive_key "$key"; then echo "$key running (pid $(pid_of "$key"))"; else echo "$key stopped"; fi
  done
  port_listening "$VNC_PORT" && echo "vnc 127.0.0.1:$VNC_PORT listening" || echo "vnc 127.0.0.1:$VNC_PORT not listening"
  port_listening "$NOVNC_PORT" && echo "novnc 127.0.0.1:$NOVNC_PORT listening" || echo "novnc 127.0.0.1:$NOVNC_PORT not listening"
  port_listening "$CONTROL_PORT" && echo "control 127.0.0.1:$CONTROL_PORT listening" || echo "control 127.0.0.1:$CONTROL_PORT not listening"
  for pt in "$VNC_PORT" "$NOVNC_PORT" "$CONTROL_PORT"; do
    w=$(wide_listener "$pt"); [ -z "$w" ] || echo "WARNING: non-loopback listener $w"
  done
}

case "$ACTION" in
  start) do_start ;;
  stop) do_stop ;;
  status) do_status ;;
esac
