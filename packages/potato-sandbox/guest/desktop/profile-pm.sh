# /etc/profile.d/pm-sandbox.sh - installed by recipes/base.json. Sourced by login shells only.
# Written by Puppet Master from the spike findings (not a Rakazo file): a login shell must never reach WSLg
# (GTK picked WSLg's Wayland socket in the spike) or the Windows display. Desktop launches also pass these
# values explicitly; this file only covers shells started with bash -l.

export XDG_RUNTIME_DIR=/tmp/pm-runtime
export GDK_BACKEND=x11 QT_QPA_PLATFORM=xcb SDL_VIDEODRIVER=x11
export DISPLAY=:99
unset WAYLAND_DISPLAY PULSE_SERVER XDG_SESSION_TYPE

# Only the desktop user creates its runtime dir, so it is never root-owned.
if [ "$(id -u)" -ne 0 ] && [ ! -d "$XDG_RUNTIME_DIR" ]; then
  mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
fi
