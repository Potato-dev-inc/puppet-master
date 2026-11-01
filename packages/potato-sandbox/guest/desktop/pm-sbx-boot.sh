#!/bin/bash
# Puppet Master original (not a Rakazo port). The one boot entry for the sandbox distro: /etc/wsl.conf has room for a single
# [boot] command, so this script runs every boot step in order, as root, and says in one place whether the sandbox is safe.
#
#   1. pm-no-wslg.sh          cut WSLg (unmount /mnt/wslg, plain /tmp/.X11-unix, no drvfs)
#   2. pm-sbx-egress.sh       firewall walls: no Windows host, no LAN, internet stays
#   3. pm-sbx-egress-probe.sh prove the walls work (dropped TCP to the gateway, internet answers)
#
# Success (all three exit 0) writes /run/pm-sbx/boot.done. Any failure stops the chain and writes /run/pm-sbx/boot.failed
# (reason=...). pm-sbx-start-desktop refuses to start unless boot.done exists, no WSLg mount is present and the probe says passed.
# WSL runs the boot command in the background, so a session started in the first seconds can run before it finishes;
# the probe waits on dropped connections, so a full boot takes tens of seconds. Wait for boot.done, never assume it.
# Log: /var/log/pm-sbx/boot.log. Idempotent: a re-run starts by removing the flags and builds them again.

# Run with a sane umask: the boot command starts with umask 0, which would make the status files world-writable.
umask 022
export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin${PATH:+:$PATH}"
SBIN=/usr/local/sbin
STATUS_DIR=/run/pm-sbx
LOG=/var/log/pm-sbx/boot.log

mkdir -p "$(dirname "$LOG")" "$STATUS_DIR" 2>/dev/null
chmod 0755 "$STATUS_DIR" 2>/dev/null
rm -f "$STATUS_DIR/boot.done" "$STATUS_DIR/boot.failed" "$STATUS_DIR/egress.status" "$STATUS_DIR/egress-probe.status"

say() { echo "$(date -Is) pm-sbx-boot: $*" >>"$LOG" 2>/dev/null; }
fail() {
  say "FAILED: $1"
  { echo "reason=$1"; echo "updated=$(date -u +%Y-%m-%dT%H:%M:%SZ)"; } >"$STATUS_DIR/boot.failed" 2>/dev/null
  exit 1
}

say "start"
for step in pm-no-wslg.sh pm-sbx-egress.sh pm-sbx-egress-probe.sh; do
  [ -f "$SBIN/$step" ] || fail "$SBIN/$step is missing"
  say "run $step"
  bash "$SBIN/$step" >>"$LOG" 2>&1 || fail "$step failed (exit $?)"
done

# The probe must have written state=passed; an exit code alone is not enough evidence.
grep -qx 'state=passed' "$STATUS_DIR/egress-probe.status" 2>/dev/null || fail "egress probe status is not passed"
if grep -q ' /mnt/wslg' /proc/mounts; then fail "WSLg is still mounted"; fi

{ echo "updated=$(date -u +%Y-%m-%dT%H:%M:%SZ)"; } >"$STATUS_DIR/boot.done"
chmod 0644 "$STATUS_DIR/boot.done"
say "done"
