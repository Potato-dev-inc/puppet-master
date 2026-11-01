#!/bin/bash
# Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file infra/compose/restrict-computer-egress.sh (the WSLg cut is from the Puppet Master spike; Rakazo has no WSLg).
# Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
# see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: this file is written for the PM distro, not Docker:
# WSLg cut-off (unmount /mnt/wslg, plain /tmp/.X11-unix, remove WSLg sockets) and no drvfs mounts. The firewall part (the CIDR lists and the
# OUTPUT chain) now lives in guest/egress/pm-sbx-egress.sh, started by pm-sbx-boot.sh. Logs to /var/log/pm-sbx/boot.log.
#
# WSLg cut for the PM sandbox distro. Run by pm-sbx-boot.sh (the one wsl.conf boot command) as root on every distro start.
# Idempotent. Exit 0 only when nothing of WSLg is reachable any more; exit 1 if a WSLg mount, socket or drvfs entry is still
# there at the end. It writes NO flag file of its own: the one verdict is /run/pm-sbx/boot.done or boot.failed, written by
# pm-sbx-boot.sh from this script's exit code. (pm-sbx-start-desktop re-checks the mounts itself.)

LOG=/var/log/pm-sbx/boot.log
mkdir -p "$(dirname "$LOG")" 2>/dev/null
exec >>"$LOG" 2>&1
echo "== $(date -Is) pm-no-wslg start"


# 1. Lazily unmount everything under /mnt/wslg (deepest first), then the mount point itself.
for m in $(awk '$2 ~ "^/mnt/wslg" {print $2}' /proc/mounts | sort -r); do
  umount -l "$m" && echo "unmounted $m"
done

# 2. /tmp/.X11-unix is a WSLg tmpfs that holds the WSLg X0 socket. Replace it with a plain sticky directory for our Xvfb.
while mountpoint -q /tmp/.X11-unix 2>/dev/null; do
  umount -l /tmp/.X11-unix && echo "unmounted /tmp/.X11-unix" || break
done
rm -f /tmp/.X11-unix/X0 2>/dev/null
[ -L /tmp/.X11-unix ] && rm -f /tmp/.X11-unix
mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix && chown root:root /tmp/.X11-unix

# 3. WSLg runtime under /run/user and any Wayland or Pulse sockets that point at WSLg.
for m in $(awk '$2 ~ "^/run/user" {print $2}' /proc/mounts | sort -r); do
  umount -l "$m" && echo "unmounted $m"
done
mkdir -p /run/user && chmod 755 /run/user
rm -f /run/user/*/wayland-0* /run/user/*/pulse/native /tmp/PulseServer 2>/dev/null
# Empty the bare mount point so nothing resolves through it.
if [ -d /mnt/wslg ] && ! mountpoint -q /mnt/wslg; then
  find /mnt/wslg -mindepth 1 -maxdepth 1 -exec rmdir {} + 2>/dev/null
fi

# 4. No Windows drive mounts from fstab (automount is already off in wsl.conf; this is defence in depth).
if grep -qs drvfs /etc/fstab; then
  sed -i '/drvfs/d' /etc/fstab && echo "removed drvfs lines from fstab"
fi

# 4b. The image ships empty mount points /mnt/c, /mnt/d ... Nothing is mounted there (automount is off); remove the empty
# folders anyway so nothing in the sandbox even names a Windows drive. rmdir only removes empty folders and refuses mount points.
for d in /mnt/[a-z]; do
  [ -d "$d" ] && ! mountpoint -q "$d" && rmdir "$d" 2>/dev/null && echo "removed empty $d"
done

# 5. Verify the cut. A leftover WSLg mount or socket means a GUI program could reach the Windows desktop.
LEFT=0
if grep -q ' /mnt/wslg' /proc/mounts; then echo "still mounted: /mnt/wslg"; LEFT=1; fi
if mountpoint -q /tmp/.X11-unix 2>/dev/null; then echo "still a mount point: /tmp/.X11-unix"; LEFT=1; fi
if [ -e /tmp/.X11-unix/X0 ]; then echo "still there: /tmp/.X11-unix/X0"; LEFT=1; fi
if grep -qs drvfs /etc/fstab || awk '$3 == "drvfs" || $3 == "9p"' /proc/mounts | grep -q .; then echo "drvfs or 9p mount present"; LEFT=1; fi
if [ "$LEFT" -ne 0 ]; then
  echo "== WSLg cut FAILED"
  exit 1
fi

echo "== done"
