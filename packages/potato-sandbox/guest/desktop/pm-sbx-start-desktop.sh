#!/bin/bash
# Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, files infra/sandboxes/computer/start.sh (Xvfb, dbus session, fluxbox config and startup, default browser, view-only x11vnc, websockify), infra/sandboxes/computer/fluxbox.init and fluxbox.menu.
# Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
# see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: runs as user pm, not PID 1 (no signal trap, no
# Docker, no supervisor loops, no Rakazo control service, no xdg portal daemons); display :99 at 1440x900; session (fluxbox or XFCE)
# from /etc/pm-sbx/desktop.conf; x11vnc and websockify bind 127.0.0.1 only and the VNC password is required; start, stop and status
# use PID files (never pkill by name); refuses to start unless WSLg is cut and the egress guard passed (/run/pm-no-wslg.done);
# fluxbox config is written into the pm state folder, not via a HOME override; menu entries use pm-sbx-browser.
#
# Usage, as user pm inside the distro:   pm-sbx-start-desktop [start|stop|status]
# Optional env: PM_SBX_DISPLAY (:99), PM_SBX_GEOMETRY (1440x900), PM_SBX_VNC_PORT (5900), PM_SBX_NOVNC_PORT (6080),
#               PM_SBX_STATE (~/.pm-sbx). The session is read from /etc/pm-sbx/desktop.conf (fluxbox by default).
# Writes: $STATE/env (source it to join the session from outside), $STATE/pids/*.pid, $STATE/logs/*.log, $STATE/vnc/{plain,passwd}.
# The VNC password is random and local. The host reads $STATE/vnc/plain through wsl when it needs to show the view.
# Note: view-only VNC means input from the screen view is refused. Take-over input goes through the control service, not VNC.

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
for p in "$VNC_PORT" "$NOVNC_PORT"; do
  if ! printf '%s\n' "$p" | grep -Eq '^[0-9]{4,5}$' || [ "$p" -lt 1024 ] || [ "$p" -gt 65535 ]; then
    echo "bad port $p (use 1024-65535)" >&2; exit 2
  fi
done
[ "$VNC_PORT" != "$NOVNC_PORT" ] || { echo "VNC and noVNC ports must differ" >&2; exit 2; }

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
  esac
}
alive_key() { alive_named "$(pid_of "$1")" "$(name_pattern "$1")"; }
port_listening() { ss -ltn 2>/dev/null | awk '{print $4}' | grep -Fqx "127.0.0.1:$1"; }
wait_port() {
  for _ in $(seq 1 50); do port_listening "$1" && return 0; sleep 0.1; done
  die "port $1 ($2) did not open; see $LOGS/$2.log" 1
}

# Refuse to touch the display unless the boot hook has cut WSLg and the egress guard passed.
gate() {
  i=0
  while [ ! -f /run/pm-no-wslg.done ] && [ ! -f /run/pm-no-wslg.failed ] && [ "$i" -lt 100 ]; do sleep 0.1; i=$((i + 1)); done
  [ -f /run/pm-no-wslg.failed ] && die "REFUSING: the egress guard failed at boot; see /var/log/pm-sbx/boot.log" 3
  [ -f /run/pm-no-wslg.done ] || die "REFUSING: boot hardening has not finished (no /run/pm-no-wslg.done)" 3
  if grep -q ' /mnt/wslg' /proc/mounts || [ -e /mnt/wslg/runtime-dir/wayland-0 ] || [ -e /tmp/.X11-unix/X0 ]; then
    die "REFUSING: WSLg is reachable from this distro" 3
  fi
}

stop_one() {
  p=$(pid_of "$1")
  if alive_named "$p" "$2"; then kill "$p" 2>/dev/null && log "stopped $1 ($p)"; fi
  rm -f "$(pidfile "$1")"
}

start_xvfb() {
  if alive_key xvfb; then log "Xvfb already running on $DISPLAY_NAME"; return 0; fi
  rm -f "/tmp/.X${DNUM}-lock" "/tmp/.X11-unix/X${DNUM}" 2>/dev/null
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
}

start_session() {
  BIN=$(session_bin)
  if alive_key session; then
    log "$BIN already running"
  else
    if [ "$SESSION" = fluxbox ]; then
      write_fluxbox_config
      xsetroot -solid "#111113" >/dev/null 2>&1 || true
      setsid nohup dbus-launch --exit-with-session fluxbox -rc "$FBDIR/init" >"$LOGS/session.log" 2>&1 </dev/null &
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

start_vnc() {
  mkdir -p "$VNCDIR" && chmod 700 "$VNCDIR"
  if [ ! -s "$VNCDIR/plain" ]; then
    # VNC authentication uses only the first 8 characters of the password.
    head -c 512 /dev/urandom | tr -dc 'A-Za-z0-9' | head -c 8 >"$VNCDIR/plain"
    chmod 600 "$VNCDIR/plain"
  fi
  if [ ! -s "$VNCDIR/passwd" ]; then
    x11vnc -storepasswd "$(cat "$VNCDIR/plain")" "$VNCDIR/passwd" >/dev/null 2>&1 || die "x11vnc could not write the password file" 1
    chmod 600 "$VNCDIR/passwd"
  fi
  if ! alive_key x11vnc; then
    setsid nohup x11vnc -display "$DISPLAY_NAME" -listen 127.0.0.1 -rfbport "$VNC_PORT" -rfbauth "$VNCDIR/passwd" \
      -viewonly -forever -shared -noxdamage -quiet -o "$LOGS/x11vnc.log" >/dev/null 2>&1 </dev/null &
    echo $! >"$(pidfile x11vnc)"
  fi
  wait_port "$VNC_PORT" x11vnc
  if ! alive_key websockify; then
    setsid nohup websockify --web /usr/share/novnc "127.0.0.1:$NOVNC_PORT" "127.0.0.1:$VNC_PORT" \
      >"$LOGS/websockify.log" 2>&1 </dev/null &
    echo $! >"$(pidfile websockify)"
  fi
  wait_port "$NOVNC_PORT" websockify
  log "VNC on 127.0.0.1:$VNC_PORT, noVNC on 127.0.0.1:$NOVNC_PORT (view only)"
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
  write_env
  echo "DESKTOP_READY session=$SESSION display=$DISPLAY_NAME seconds=$(( $(date +%s) - T0 ))"
}

do_stop() {
  stop_one websockify websockify
  stop_one x11vnc x11vnc
  stop_one session "$(session_bin)"
  stop_one xvfb Xvfb
  echo "DESKTOP_STOPPED"
}

do_status() {
  for key in xvfb session x11vnc websockify; do
    if alive_key "$key"; then echo "$key running (pid $(pid_of "$key"))"; else echo "$key stopped"; fi
  done
  port_listening "$VNC_PORT" && echo "vnc 127.0.0.1:$VNC_PORT listening" || echo "vnc 127.0.0.1:$VNC_PORT not listening"
  port_listening "$NOVNC_PORT" && echo "novnc 127.0.0.1:$NOVNC_PORT listening" || echo "novnc 127.0.0.1:$NOVNC_PORT not listening"
}

case "$ACTION" in
  start) do_start ;;
  stop) do_stop ;;
  status) do_status ;;
esac
