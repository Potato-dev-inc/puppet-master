#!/bin/bash
# The "Zorin look" for the sandbox's XFCE desktop. Puppet Master (not a Rakazo file). Runs once, as the desktop user, from the
# XDG autostart entry that the zorin-look recipe installs (/etc/xdg/autostart/pm-sbx-zorin-look.desktop). It sets the same
# xfconf keys that the spike proved by hand (D:\pm-sandbox\spike\zorin\look.sh): the Zorin light theme and icons, the Inter
# font, a Zorin wallpaper, the window buttons on the right and a 40 px panel at the bottom.
#
# The themes, icons, wallpapers and font are plain files from four Zorin packages. No Zorin panel, menu or session is used.
# A step that fails is skipped (the desktop still works); the mark file is written at the end so this does not run again.

MARK="$HOME/.config/pm-sbx/zorin-look.done"
[ -e "$MARK" ] && exit 0

# Wait (up to 20 s) for the desktop service and the settings daemon of this session.
for _ in $(seq 1 100); do
  if pgrep -u "$(id -un)" -x xfdesktop >/dev/null 2>&1 && xfconf-query -c xsettings -l >/dev/null 2>&1; then break; fi
  sleep 0.2
done

# Sets one key, creating it when it does not exist yet.
X() { xfconf-query -c "$1" -p "$2" -n -t "$3" -s "$4" 2>/dev/null || xfconf-query -c "$1" -p "$2" -s "$4" 2>/dev/null; }

X xsettings /Net/ThemeName string ZorinBlue-Light
X xsettings /Net/IconThemeName string ZorinBlue-Light
X xsettings /Gtk/FontName string "Inter 10"
X xfwm4 /general/theme string ZorinBlue-Light
X xfwm4 /general/title_font string "Inter Bold 10"
X xfwm4 /general/button_layout string "|HMC"
# No compositor: the sandbox draws on a software X server.
X xfwm4 /general/use_compositing bool false

# A Zorin wallpaper: the one the spike used, else the first picture that the wallpaper package installed.
WP=/usr/share/backgrounds/Dolomites-by-Wirestock.jpg
if [ ! -f "$WP" ]; then
  WP=$(dpkg -L zorin-os-wallpapers-18 2>/dev/null | grep -iE '\.(jpe?g|png|webp)$' | head -1)
fi
if [ -n "$WP" ] && [ -f "$WP" ]; then
  for p in $(xfconf-query -c xfce4-desktop -l 2>/dev/null | grep last-image); do xfconf-query -c xfce4-desktop -p "$p" -s "$WP"; done
  # 5 = zoomed: the picture fills the screen.
  for p in $(xfconf-query -c xfce4-desktop -l 2>/dev/null | grep image-style); do xfconf-query -c xfce4-desktop -p "$p" -s 5; done
fi

# The panel at the bottom, a little taller, like a taskbar.
xfconf-query -c xfce4-panel -p /panels/panel-1/position -s "p=10;x=0;y=0" 2>/dev/null
xfconf-query -c xfce4-panel -p /panels/panel-1/size -s 40 2>/dev/null
xfconf-query -c xfce4-panel -p /panels/panel-1/length -s 100 2>/dev/null
xfconf-query -c xfce4-panel -p /panels/panel-1/position-locked -s true 2>/dev/null

xfce4-panel -r >/dev/null 2>&1 &
xfdesktop --reload >/dev/null 2>&1 &

mkdir -p "$(dirname "$MARK")" 2>/dev/null
date -Is >"$MARK" 2>/dev/null
exit 0
