#!/bin/bash
# Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file infra/compose/restrict-computer-egress.sh (the IPv4 and IPv6 CIDR lists only).
# Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
# see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: the rest of this file is written for the PM distro,
# not Docker: WSLg cut-off (unmount /mnt/wslg, plain /tmp/.X11-unix, remove WSLg sockets), no drvfs mounts, and an OUTPUT chain
# (not DOCKER-USER) that also blocks the WSL gateway, followed by a probe that must fail closed. Logs to /var/log/pm-sbx/boot.log.
#
# Boot hardening for the PM sandbox distro. Runs as root on every distro start from wsl.conf ([boot] command=).
# Idempotent. Writes /run/pm-no-wslg.done only when WSLg is cut AND the egress guard passed its probe.
# pm-sbx-start-desktop refuses to start any GUI unless that flag exists.
# Without systemd this script is the only boot hook; the host keeps the distro alive with a hidden holder process.

LOG=/var/log/pm-sbx/boot.log
mkdir -p "$(dirname "$LOG")" 2>/dev/null
exec >>"$LOG" 2>&1
echo "== $(date -Is) pm-no-wslg start"

# Flag names are fixed: pm-sbx-start-desktop waits for the .done flag and refuses without it.
rm -f /run/pm-no-wslg.done /run/pm-no-wslg.failed

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

# 5. Egress guard. The sandbox keeps the internet (web, apt, git) but may not reach the Windows host,
# the LAN, CGNAT, link-local, multicast or reserved space. The WSL gateway is the Windows host, so it is blocked too.
# Fails closed: no iptables, or a probe that connects, means no GUI.
EGRESS_OK=1
if ! command -v iptables >/dev/null 2>&1; then
  echo "iptables missing: egress guard not applied"
  EGRESS_OK=0
else
  GW=$(ip route show default 2>/dev/null | awk '{print $3; exit}')
  iptables -w -N PM_EGRESS 2>/dev/null
  iptables -w -F PM_EGRESS
  iptables -w -A PM_EGRESS -o lo -j ACCEPT
  iptables -w -A PM_EGRESS -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
  iptables -w -A PM_EGRESS -d 10.255.255.254 -j ACCEPT   # WSL DNS tunnel address; must come before the 10/8 drop
  for net in 0.0.0.0/8 10.0.0.0/8 100.64.0.0/10 127.0.0.0/8 169.254.0.0/16 172.16.0.0/12 192.0.0.0/24 192.168.0.0/16 198.18.0.0/15 224.0.0.0/4 240.0.0.0/4; do
    iptables -w -A PM_EGRESS -d "$net" -j REJECT
  done
  [ -n "$GW" ] && iptables -w -A PM_EGRESS -d "$GW" -j REJECT
  iptables -w -C OUTPUT -j PM_EGRESS 2>/dev/null || iptables -w -I OUTPUT 1 -j PM_EGRESS
  echo "egress guard v4 on (gateway=${GW:-none})"

  if command -v ip6tables >/dev/null 2>&1; then
    ip6tables -w -N PM_EGRESS6 2>/dev/null
    ip6tables -w -F PM_EGRESS6
    ip6tables -w -A PM_EGRESS6 -o lo -j ACCEPT
    ip6tables -w -A PM_EGRESS6 -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
    for net in fc00::/7 fe80::/10 ff00::/8; do
      ip6tables -w -A PM_EGRESS6 -d "$net" -j REJECT
    done
    ip6tables -w -C OUTPUT -j PM_EGRESS6 2>/dev/null || ip6tables -w -I OUTPUT 1 -j PM_EGRESS6
    echo "egress guard v6 on"
  fi

  # Probe: the Windows host must NOT answer on SMB (445), RPC (135) or RDP (3389) from the gateway address.
  # A connection that succeeds means the guard is not working.
  if [ -n "$GW" ]; then
    for port in 445 135 3389; do
      if timeout 2 bash -c "exec 3<>/dev/tcp/$GW/$port" 2>/dev/null; then
        echo "PROBE FAILED: reached $GW:$port through the guard"
        EGRESS_OK=0
      fi
    done
  fi
fi

if [ "$EGRESS_OK" -ne 1 ]; then
  touch /run/pm-no-wslg.failed
  echo "== WSLg cut done, but the egress guard failed: desktop will not start"
  exit 1
fi

touch /run/pm-no-wslg.done
echo "== done"
