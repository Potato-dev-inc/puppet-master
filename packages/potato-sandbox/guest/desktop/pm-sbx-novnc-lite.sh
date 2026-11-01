#!/bin/bash
# pm-sbx-novnc-lite.sh - puts the stock noVNC web pages (/usr/share/novnc) in place WITHOUT the packages Ubuntu's novnc
# package drags in (nodejs, python3-novnc with the OpenStack oslo.* libraries, python3-pil, babel locale data, netaddr:
# about 60 packages, 65 MB of downloads, none of which noVNC's static pages or websockify use).
# Puppet Master original. Run as root, after apt works (recipes/base.json post line). Safe to run again.
#
# How: apt-get download novnc (signature-checked by apt like any package), open the .deb, drop its Depends/Pre-Depends/
# Recommends lines, give it the local version suffix +pmsbx1, install that with dpkg and hold it. dpkg then knows the
# files (apt remove novnc works) and apt never asks for the missing dependencies.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

if dpkg-query -W -f='${Version}\n' novnc 2>/dev/null | grep -q '+pmsbx'; then
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -r -- "$work"' EXIT
cd "$work"
apt-get download novnc
dpkg-deb -R novnc_*.deb pkg
python3 -I - pkg/DEBIAN/control <<'PY'
import re
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as fh:
    text = fh.read()
out = []
skip = False
for line in text.split("\n"):
    if line.startswith((" ", "\t")):          # a continuation line belongs to the field above
        if not skip:
            out.append(line)
        continue
    skip = bool(re.match(r"(Pre-Depends|Depends|Recommends):", line))
    if skip:
        continue
    if line.startswith("Version:"):
        line = line.rstrip() + "+pmsbx1"
    out.append(line)
with open(path, "w", encoding="utf-8") as fh:
    fh.write("\n".join(out))
PY
dpkg-deb --root-owner-group -b pkg novnc-lite.deb >/dev/null
dpkg -i --force-unsafe-io novnc-lite.deb
apt-mark hold novnc >/dev/null
test -f /usr/share/novnc/vnc.html
