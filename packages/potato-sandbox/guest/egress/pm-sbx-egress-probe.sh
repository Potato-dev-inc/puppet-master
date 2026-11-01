#!/usr/bin/env bash
# Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file infra/sandboxes/supervisor/src/egress-probe.ts.
# Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
# see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: rewritten as a shell check for the WSL guest (no probe container); checks the WSL gateway and private resolvers on Windows ports, the metadata address and https://example.com; detects mirrored networking; reads the status file written by pm-sbx-egress.sh; writes a probe status file.
#
# Checks that the walls set by pm-sbx-egress.sh really work. Run it after that script,
# at boot:
#
#   bash pm-sbx-egress-probe.sh
#
# Exit 0: the walls hold and the internet answers.
# Exit 1: something is wrong. The reason is printed to stderr and written to
#         /run/pm-sbx/egress-probe.status (state=passed or state=failed).
#
# Checks, in order:
#   1. pm-sbx-egress.sh reported state=applied.
#   2. Not mirrored networking: a separate WSL gateway exists, it is not one of this
#      guest's own addresses, it is inside 172.16.0.0/12 (the WSL NAT range), and no
#      guest address is listed in /etc/pm-sbx/host-addresses (optional, one IPv4 per line).
#   3. TCP to the gateway, and to any private resolver, on ports 135, 445, 3389, 5357
#      and one random high port: each must time out (dropped). A refusal or a quick
#      error counts as a failure, because it means the host answered.
#   4. TCP to 169.254.169.254:80 (cloud metadata) must time out.
#   5. https://example.com must answer (curl, three tries). Without curl, a TCP connect
#      to example.com:443 must work.

set -Eeuo pipefail
# A boot command may start with a short PATH; ip lives in sbin.
export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin${PATH:+:$PATH}"

IP="${PM_SBX_IP:-ip}"
CURL="${PM_SBX_CURL:-curl}"
RESOLV_CONF="${PM_SBX_RESOLV_CONF:-/etc/resolv.conf}"
RESOLVED_CONF="${PM_SBX_RESOLVED_CONF:-/run/systemd/resolve/resolv.conf}"
STATUS_DIR="${PM_SBX_STATUS_DIR:-/run/pm-sbx}"
EGRESS_STATUS="$STATUS_DIR/egress.status"
PROBE_STATUS="$STATUS_DIR/egress-probe.status"
HOST_ADDRESS_FILE="${PM_SBX_HOST_ADDRESS_FILE:-/etc/pm-sbx/host-addresses}"
TCP_SECONDS=2
INTERNET_ATTEMPTS=3
METADATA_ADDRESS=169.254.169.254
WINDOWS_PORTS=(135 445 3389 5357)
BLOCKED_V4=(
  0.0.0.0/8 10.0.0.0/8 100.64.0.0/10 127.0.0.0/8 169.254.0.0/16 172.16.0.0/12
  192.0.0.0/24 192.168.0.0/16 198.18.0.0/15 224.0.0.0/4 240.0.0.0/4
)

GATEWAYS=()
NAMESERVERS=()

log() { echo "pm-sbx-egress-probe: $*" >&2; }

write_probe_status() { # $1 = state, $2 = reason (optional)
  local tmp="$PROBE_STATUS.tmp"
  mkdir -p "$STATUS_DIR" 2>/dev/null || return 0
  {
    echo "state=$1"
    if [[ -n ${2:-} ]]; then echo "reason=$2"; fi
    echo "updated=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  } >"$tmp" 2>/dev/null && mv -f "$tmp" "$PROBE_STATUS" 2>/dev/null || true
}

fail_probe() {
  log "FAIL: $*"
  write_probe_status failed "$*"
  exit 1
}

on_error() {
  fail_probe "unexpected error at line $1"
}

is_ipv4() { [[ $1 =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]]; }

ipv4_int() {
  local a b c d
  IFS=. read -r a b c d <<<"$1"
  echo $(((a << 24) | (b << 16) | (c << 8) | d))
}

# True when address $1 is inside CIDR $2.
ipv4_in_cidr() {
  local net=${2%/*} bits=${2#*/} mask
  mask=$(((0xFFFFFFFF << (32 - bits)) & 0xFFFFFFFF))
  (((($(ipv4_int "$1") & mask)) == ($(ipv4_int "$net") & mask)))
}

is_blocked_v4() {
  local cidr
  for cidr in "${BLOCKED_V4[@]}"; do
    if ipv4_in_cidr "$1" "$cidr"; then
      return 0
    fi
  done
  return 1
}

default_gateways_v4() {
  { "$IP" -4 route show default 2>/dev/null || true; } |
    awk '$1 == "default" { for (i = 2; i < NF; i++) if ($i == "via") print $(i + 1) }' |
    sort -u
}

local_ipv4() {
  { "$IP" -o -4 addr show 2>/dev/null || true; } |
    awk '{ for (i = 1; i < NF; i++) if ($i == "inet") { sub(/\/.*/, "", $(i + 1)); print $(i + 1) } }'
}

# IPv4 nameservers from resolv.conf and the systemd-resolved upstream list, without duplicates.
nameservers_v4() {
  local file ns
  for file in "$RESOLV_CONF" "$RESOLVED_CONF"; do
    if [[ -r $file ]]; then
      awk '$1 == "nameserver" && NF >= 2 { print $2 }' "$file"
    fi
  done | sort -u | while IFS= read -r ns; do
    if is_ipv4 "$ns"; then
      echo "$ns"
    fi
  done
}

check_egress_applied() {
  if [[ ! -r $EGRESS_STATUS ]]; then
    fail_probe "egress rules not applied: $EGRESS_STATUS is missing (run pm-sbx-egress.sh first)"
  fi
  if ! grep -qx 'state=applied' "$EGRESS_STATUS"; then
    fail_probe "egress rules not applied (see $EGRESS_STATUS)"
  fi
  log "ok: egress rules are applied"
}

check_not_mirrored() {
  local gw addr host
  local -a locals=()
  if ((${#GATEWAYS[@]} == 0)); then
    fail_probe "mirrored networking not supported: no separate WSL gateway (no IPv4 default route)"
  fi
  mapfile -t locals < <(local_ipv4)
  for gw in "${GATEWAYS[@]}"; do
    for addr in "${locals[@]}"; do
      if [[ $gw == "$addr" ]]; then
        fail_probe "mirrored networking not supported: the default gateway $gw is this guest's own address"
      fi
    done
    if ! ipv4_in_cidr "$gw" 172.16.0.0/12; then
      fail_probe "mirrored networking not supported: the default gateway $gw is outside the WSL NAT range 172.16.0.0/12"
    fi
  done
  if [[ -r $HOST_ADDRESS_FILE ]]; then
    while IFS= read -r host; do
      host=${host%%#*}
      host=${host//[[:space:]]/}
      if [[ -z $host ]]; then
        continue
      fi
      for addr in "${locals[@]}"; do
        if [[ $addr == "$host" ]]; then
          fail_probe "mirrored networking not supported: guest address $addr is a Windows host address"
        fi
      done
    done <"$HOST_ADDRESS_FILE"
  fi
  log "ok: NAT networking (gateway ${GATEWAYS[*]}), not mirrored"
}

# One TCP connect, bounded by timeout. Exit 0 = connected, 124 = timed out, else refused/error.
tcp_connect() { # $1 = host, $2 = port, $3 = seconds
  timeout "$3" bash -c 'exec 3<>"/dev/tcp/$0/$1"' "$1" "$2" 2>/dev/null
}

# The wall works only when the connect times out. Anything else means the host answered
# (or the path is broken), and either way the probe fails.
expect_dropped() { # $1 = host, $2 = port
  local rc=0
  tcp_connect "$1" "$2" "$TCP_SECONDS" || rc=$?
  if ((rc == 124)); then
    return 0
  fi
  if ((rc == 0)); then
    fail_probe "walls not working: TCP to $1:$2 connected (the host answered)"
  fi
  fail_probe "walls not working: TCP to $1:$2 got a quick error (exit $rc), not a drop"
}

check_walls() {
  local host port ns high
  local -a targets=()
  high=$((49152 + RANDOM % 16384))
  for host in "${GATEWAYS[@]}"; do
    targets+=("$host")
  done
  for ns in "${NAMESERVERS[@]}"; do
    if is_blocked_v4 "$ns" && [[ " ${targets[*]} " != *" $ns "* ]]; then
      targets+=("$ns")
    fi
  done
  for host in "${targets[@]}"; do
    for port in "${WINDOWS_PORTS[@]}" "$high"; do
      expect_dropped "$host" "$port"
    done
  done
  log "ok: TCP to ${targets[*]} dropped on ${WINDOWS_PORTS[*]} and $high"
}

check_metadata() {
  expect_dropped "$METADATA_ADDRESS" 80
  log "ok: TCP to $METADATA_ADDRESS:80 dropped"
}

check_internet() {
  local attempt err last
  if command -v "$CURL" >/dev/null 2>&1; then
    for ((attempt = 1; attempt <= INTERNET_ATTEMPTS; attempt++)); do
      if err="$("$CURL" -sS -o /dev/null --max-time 15 https://example.com/ 2>&1)"; then
        log "ok: https://example.com answered"
        return 0
      fi
      if ((attempt < INTERNET_ATTEMPTS)); then
        sleep 5
      fi
    done
    last=${err##*$'\n'}
    fail_probe "internet: https://example.com did not answer after $INTERNET_ATTEMPTS tries (${last:-curl gave no message})"
  fi
  log "note: curl is not installed; checking a TCP connect to example.com:443 instead"
  for ((attempt = 1; attempt <= INTERNET_ATTEMPTS; attempt++)); do
    if tcp_connect example.com 443 10; then
      log "ok: TCP to example.com:443 connected"
      return 0
    fi
    if ((attempt < INTERNET_ATTEMPTS)); then
      sleep 5
    fi
  done
  fail_probe "internet: TCP to example.com:443 did not connect after $INTERNET_ATTEMPTS tries"
}

main() {
  trap 'on_error $LINENO' ERR
  check_egress_applied
  mapfile -t GATEWAYS < <(default_gateways_v4)
  mapfile -t NAMESERVERS < <(nameservers_v4)
  check_not_mirrored
  check_walls
  check_metadata
  check_internet
  write_probe_status passed
  log "PASS: walls hold and the internet answers"
}

main "$@"
