#!/usr/bin/env bash
# Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file infra/compose/restrict-computer-egress.sh.
# Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
# see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: guards this WSL2 guest's own OUTPUT path instead of Docker's DOCKER-USER forward path; no Docker, bridge or systemd parts; runs as root from the wsl.conf boot command; iptables with an nft fallback; DNS allowed only to the resolvers in resolv.conf; WSL default gateway blocked by address; rules built in a new chain and swapped in; waits for the default route at boot; writes a status file.
#
# Restricted egress for the Puppet Master sandbox guest (one PM-owned WSL2 distro).
#
# The guest may use the public internet (browsing, apt, git, DNS). It may not reach:
#   - the Windows host: the WSL default gateway, and the 10.255.255.254 DNS tunnel,
#   - the LAN, CGNAT, loopback other than lo, link-local, cloud metadata,
#     multicast and reserved space,
#   - IPv6 unique-local, link-local or multicast space, except the link-local
#     ICMPv6 that neighbour and router discovery need.
# DNS (port 53, UDP and TCP) is allowed only to the servers in resolv.conf, even
# when such a server has a private address.
#
# Runs as root at distro boot. /etc/wsl.conf has one [boot] command, /usr/local/sbin/pm-sbx-boot.sh, which runs
# pm-no-wslg.sh, then this script, then pm-sbx-egress-probe.sh. The gateway may be in any private range
# (172.16/12, 192.168/16 or 10/8 on different PCs); it is read from the default route, never assumed.
#
#   bash pm-sbx-egress.sh            apply the rules (what the boot command runs)
#   bash pm-sbx-egress.sh --print   print the iptables commands, change nothing
#
# The rules live in their own chain (PM_SBX_EGRESS). A new copy is built beside the
# old one and swapped in, so a re-run is safe and a failure part-way never leaves
# the walls half built. The result goes to /run/pm-sbx/egress.status.
# pm-sbx-egress-probe.sh checks that the walls really work.

set -Eeuo pipefail
# A boot command may start with a short PATH; iptables, ip and nft live in sbin.
export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin${PATH:+:$PATH}"

IPT="${PM_SBX_IPTABLES:-iptables}"
IP6T="${PM_SBX_IP6TABLES:-ip6tables}"
NFT="${PM_SBX_NFT:-nft}"
IP="${PM_SBX_IP:-ip}"
RESOLV_CONF="${PM_SBX_RESOLV_CONF:-/etc/resolv.conf}"
RESOLVED_CONF="${PM_SBX_RESOLVED_CONF:-/run/systemd/resolve/resolv.conf}"
IF_INET6="${PM_SBX_IF_INET6:-/proc/net/if_inet6}"
STATUS_DIR="${PM_SBX_STATUS_DIR:-/run/pm-sbx}"
STATUS_FILE="$STATUS_DIR/egress.status"
GATEWAY_WAIT_SECONDS="${PM_SBX_GATEWAY_WAIT:-30}"
CHAIN=PM_SBX_EGRESS
CHAIN_NEW=PM_SBX_EGRESS_NEW
NFT_TABLE=pm_sbx_egress

# Facts found during a run. write_status reports them.
STATUS_BACKEND=none
STATUS_CHAIN=none
STATUS_V6=none
GATEWAYS_V4=()
GATEWAYS_V6=()
RESOLVERS_V4=()
RESOLVERS_V6=()

usage() {
  cat <<'EOF'
Usage: pm-sbx-egress.sh [apply|--print]
  apply (default)  build the PM_SBX_EGRESS rules and write /run/pm-sbx/egress.status
  --print          print the iptables commands that apply would run; change nothing
EOF
}

log() { echo "pm-sbx-egress: $*" >&2; }

# Blocked destinations, checked after the loopback, established and DNS rules.
blocked_list() {
  if [[ $1 == 4 ]]; then
    cat <<'EOF'
0.0.0.0/8
10.0.0.0/8
100.64.0.0/10
127.0.0.0/8
169.254.0.0/16
172.16.0.0/12
192.0.0.0/24
192.168.0.0/16
198.18.0.0/15
224.0.0.0/4
240.0.0.0/4
EOF
  else
    # ::1/128 here means loopback that is not on lo. ff00::/8 also covers ff02::/16,
    # which iptables_specs exempts for ICMPv6 before this list is read.
    cat <<'EOF'
::1/128
fc00::/7
fe80::/10
ff00::/8
EOF
  fi
}

is_ipv4() { [[ $1 =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]]; }
is_ipv6() { [[ $1 == *:* && $1 =~ ^[0-9A-Fa-f:.]+$ ]]; }

write_status() { # $1 = state, $2 = reason (optional)
  local tmp="$STATUS_FILE.tmp"
  mkdir -p "$STATUS_DIR" 2>/dev/null || return 0
  {
    echo "state=$1"
    if [[ -n ${2:-} ]]; then echo "reason=$2"; fi
    echo "backend=$STATUS_BACKEND"
    echo "chain=$STATUS_CHAIN"
    echo "ipv6=$STATUS_V6"
    echo "gateways_v4=${GATEWAYS_V4[*]:-}"
    echo "gateways_v6=${GATEWAYS_V6[*]:-}"
    echo "resolvers_v4=${RESOLVERS_V4[*]:-}"
    echo "resolvers_v6=${RESOLVERS_V6[*]:-}"
    echo "updated=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  } >"$tmp" 2>/dev/null && mv -f "$tmp" "$STATUS_FILE" 2>/dev/null || true
}

fail() {
  log "$*"
  write_status failed "$*"
  exit 1
}

on_error() {
  log "stopped at line $1 (a command failed)"
  write_status failed "command failed at line $1 of pm-sbx-egress.sh"
  exit 1
}

require_root() {
  if ((EUID != 0)) && [[ ${PM_SBX_SKIP_ROOT_CHECK:-0} != 1 ]]; then
    fail "must run as root (the WSL boot command runs as root)"
  fi
}

# Default-route gateways for one family ($1 = -4 or -6).
default_gateways() {
  { "$IP" "$1" route show default 2>/dev/null || true; } |
    awk '$1 == "default" { for (i = 2; i < NF; i++) if ($i == "via") print $(i + 1) }' |
    sort -u
}

# WSL writes its default route when the distro starts. Wait a little for it, because
# without the IPv4 gateway the Windows host cannot be blocked by name.
wait_for_gateway() {
  local deadline=$((SECONDS + GATEWAY_WAIT_SECONDS))
  while :; do
    mapfile -t GATEWAYS_V4 < <(default_gateways -4)
    if ((${#GATEWAYS_V4[@]} > 0)); then
      return 0
    fi
    if ((SECONDS >= deadline)); then
      return 1
    fi
    sleep 1
  done
}

# Nameservers from resolv.conf and, when systemd-resolved runs, its upstream list.
read_resolvers() {
  local file ns seen=" "
  local -a all=()
  for file in "$RESOLV_CONF" "$RESOLVED_CONF"; do
    if [[ -r $file ]]; then
      while IFS= read -r ns; do
        all+=("$ns")
      done < <(awk '$1 == "nameserver" && NF >= 2 { print $2 }' "$file")
    fi
  done
  for ns in "${all[@]}"; do
    if [[ $seen == *" $ns "* ]]; then
      continue
    fi
    seen+="$ns "
    if is_ipv4 "$ns"; then
      RESOLVERS_V4+=("$ns")
    elif is_ipv6 "$ns"; then
      RESOLVERS_V6+=("$ns")
    else
      log "ignoring resolv.conf entry '$ns' (not a plain IP address)"
    fi
  done
  if ((${#RESOLVERS_V4[@]} == 0 && ${#RESOLVERS_V6[@]} == 0)); then
    log "warning: no nameserver found; DNS will not work"
  fi
}

# iptables rule specs for one family ($1 = 4 or 6), in chain order. Each line is
# appended with -A, so the order printed here is the order in the chain.
iptables_specs() {
  local fam=$1 ns gw cidr
  local -n res="RESOLVERS_V$fam" gws="GATEWAYS_V$fam"
  echo "-o lo -j ACCEPT"
  echo "-m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT"
  for ns in "${res[@]}"; do
    echo "-d $ns -p udp --dport 53 -j ACCEPT"
    echo "-d $ns -p tcp --dport 53 -j ACCEPT"
    echo "-d $ns -j DROP"
  done
  for gw in "${gws[@]}"; do
    echo "-d $gw -j DROP"
  done
  if [[ $fam == 6 ]]; then
    # Neighbour and router discovery send ICMPv6 to ff02::/16. It never leaves the link.
    echo "-d ff02::/16 -p icmpv6 -j ACCEPT"
  fi
  while IFS= read -r cidr; do
    echo "-d $cidr -j DROP"
  done < <(blocked_list "$fam")
}

# nft rules for one family ($1 = 4 or 6). The caller adds the lo and established rules.
nft_rules() {
  local fam=$1 ns gw cidr p=ip
  local -n res="RESOLVERS_V$fam" gws="GATEWAYS_V$fam"
  if [[ $fam == 6 ]]; then p=ip6; fi
  for ns in "${res[@]}"; do
    echo "$p daddr $ns udp dport 53 accept"
    echo "$p daddr $ns tcp dport 53 accept"
    echo "$p daddr $ns drop"
  done
  for gw in "${gws[@]}"; do
    echo "$p daddr $gw drop"
  done
  if [[ $fam == 6 ]]; then
    echo "ip6 daddr ff02::/16 meta l4proto icmpv6 accept"
  fi
  while IFS= read -r cidr; do
    echo "$p daddr $cidr drop"
  done < <(blocked_list "$fam")
}

# One iptables family ($1 = command, $2 = family). Builds PM_SBX_EGRESS_NEW, checks it,
# then moves the OUTPUT jump to it. The old chain stays live until the new one is full.
apply_iptables_family() {
  local cmd=$1 fam=$2 line n
  local -a rules=() args=()
  mapfile -t rules < <(iptables_specs "$fam")
  "$cmd" -N "$CHAIN_NEW" 2>/dev/null || true
  "$cmd" -F "$CHAIN_NEW"
  for line in "${rules[@]}"; do
    read -ra args <<<"$line"
    "$cmd" -A "$CHAIN_NEW" "${args[@]}"
  done
  n="$("$cmd" -S "$CHAIN_NEW" | grep -c '^-A ' || true)"
  if [[ $n != "${#rules[@]}" ]]; then
    fail "$cmd: $CHAIN_NEW has $n rules, expected ${#rules[@]}"
  fi
  # Leftover jumps from an earlier failed run go; then the new jump goes to the top.
  while "$cmd" -D OUTPUT -j "$CHAIN_NEW" 2>/dev/null; do :; done
  "$cmd" -I OUTPUT 1 -j "$CHAIN_NEW"
  # Old jumps and the old chain go; the new chain takes the name PM_SBX_EGRESS.
  while "$cmd" -D OUTPUT -j "$CHAIN" 2>/dev/null; do :; done
  "$cmd" -F "$CHAIN" 2>/dev/null || true
  "$cmd" -X "$CHAIN" 2>/dev/null || true
  "$cmd" -E "$CHAIN_NEW" "$CHAIN"
}

host_has_ipv6_addresses() {
  [[ -s $IF_INET6 ]]
}

apply_iptables_backend() {
  STATUS_BACKEND=iptables
  STATUS_CHAIN=$CHAIN
  apply_iptables_family "$IPT" 4
  if command -v "$IP6T" >/dev/null 2>&1 && "$IP6T" -S >/dev/null 2>&1; then
    apply_iptables_family "$IP6T" 6
    STATUS_V6=iptables
  elif host_has_ipv6_addresses; then
    fail "$IP6T is missing or unusable, but this guest has IPv6 addresses; the IPv6 walls cannot be applied"
  else
    STATUS_V6=disabled
  fi
}

# nft fallback for hosts without iptables. One file is one transaction: the table is
# created if missing, deleted, and written again, all or nothing.
apply_nft_backend() {
  local file="$STATUS_DIR/egress.nft"
  STATUS_BACKEND=nft
  STATUS_CHAIN="inet $NFT_TABLE"
  STATUS_V6=nft
  {
    echo "add table inet $NFT_TABLE"
    echo "delete table inet $NFT_TABLE"
    echo "table inet $NFT_TABLE {"
    echo "  chain output {"
    echo "    type filter hook output priority 0; policy accept;"
    echo "    oifname \"lo\" accept"
    echo "    ct state established,related accept"
    nft_rules 4 | sed 's/^/    /'
    nft_rules 6 | sed 's/^/    /'
    echo "  }"
    echo "}"
  } >"$file"
  "$NFT" -f "$file"
  rm -f "$file"
}

apply_rules() {
  require_root
  mkdir -p "$STATUS_DIR"
  if ! wait_for_gateway; then
    fail "no IPv4 default gateway (the Windows host address) after ${GATEWAY_WAIT_SECONDS}s"
  fi
  mapfile -t GATEWAYS_V6 < <(default_gateways -6)
  read_resolvers
  if command -v "$IPT" >/dev/null 2>&1 && "$IPT" -S >/dev/null 2>&1; then
    apply_iptables_backend
  elif command -v "$NFT" >/dev/null 2>&1 && "$NFT" list ruleset >/dev/null 2>&1; then
    apply_nft_backend
  else
    fail "no usable firewall backend: need iptables (iptables-nft is fine) or nft"
  fi
  write_status applied
  log "egress rules applied (backend $STATUS_BACKEND, ipv6 $STATUS_V6)"
}

# Print the iptables commands that apply would run. No root, no changes.
print_rules() {
  local fam spec cmd
  mapfile -t GATEWAYS_V4 < <(default_gateways -4)
  mapfile -t GATEWAYS_V6 < <(default_gateways -6)
  read_resolvers
  for fam in 4 6; do
    cmd=$IPT
    if [[ $fam == 6 ]]; then cmd=$IP6T; fi
    while IFS= read -r spec; do
      echo "$cmd -A $CHAIN $spec"
    done < <(iptables_specs "$fam")
  done
}

case "${1:-apply}" in
  apply)
    trap 'on_error $LINENO' ERR
    apply_rules
    ;;
  --print)
    print_rules
    ;;
  -h | --help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
