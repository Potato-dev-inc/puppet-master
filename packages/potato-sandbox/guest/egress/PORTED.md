# PORTED.md: guest egress walls

Source commit: f48fa5c7 (Apache-2.0). Line counts are approximate. This is a rewrite of the
logic for one WSL2 guest, not a line-by-line port.

| our file | their file | what changed | lines kept/removed |
|---|---|---|---|
| `pm-sbx-egress.sh` (346 lines) | `infra/compose/restrict-computer-egress.sh` (388 lines) | Rewritten for the guest's own OUTPUT path (chain `PM_SBX_EGRESS`), not Docker's DOCKER-USER forward path. Kept: the 11 IPv4 blocked CIDRs (all) and the 4 IPv6 blocked entries (all, `::1/128` included; it only matters off `lo`), the root check, the `--print` mode, the usage text. Removed: Docker bridge-name rules (`<prefix>-c+`), the INPUT rules, the Docker wait, the systemd unit install and remove, and the prefix and stale-rule repair (replaced by a build-then-swap of a whole new chain). Added: DNS allowed only to the `resolv.conf` servers on port 53 with all other traffic to them dropped; the WSL default gateway blocked by address; `iptables` first with an `nft` fallback; ip6tables required when IPv6 addresses exist; IPv6 `ff02::/16` ICMPv6 exception; wait up to 30 s for the default route at boot; status file; full PATH at start. | about 45 kept, about 340 removed or replaced |
| `pm-sbx-egress-probe.sh` (252 lines) | `infra/sandboxes/supervisor/src/egress-probe.ts` (154 lines) | Rewritten as a shell check; no probe container or Docker network. Kept: the rule that only a timeout proves a drop, the cloud metadata address check, and the idea of a DNS-answer check (now an https check). Changed: Windows ports 135, 445, 3389, 5357 and one random high port on the gateway and private resolvers (their ports were 22 and 1). Added: mirrored networking detection (no gateway, gateway is a guest address, gateway that is not a private (RFC 1918 or CGNAT) address, guest address listed in an optional host-address file), a check that the apply script reported `state=applied`, a curl internet check with retries (TCP fallback without curl), a probe status file. | about 25 kept, about 130 removed |

## Not ported

- `infra/compose/restrict-computer-egress.smoke.sh`: a test for their script (it stubs iptables and
  checks their rule text). Not shipped. Our own stub tests cover our script, and they live outside this folder.
- `packages/core/src/egress.ts`: decides which site a request may trust (domain and suffix rules).
  It is application policy, not a firewall. It does not fit this folder.

## Where we differ on purpose

- Metadata check: their rule fails only when metadata accepts a connection. Ours fails on any
  connection or quick error, because a wall should drop, not answer. In a WSL guest with the walls
  in place the connect times out, so this passes there.
- ICMPv6 to `ff02::/16` is allowed. Their ruleset dropped `ff00::/8` on the forward path, where
  locally sent neighbour discovery does not pass. Our OUTPUT rules would break IPv6 neighbour
  discovery without this exception.
- The WSL gateway is blocked by address, as the brief asks. On PCs where WSL picks 172.16.0.0/12, 192.168.0.0/16 or 10.0.0.0/8
  the range rule also covers it; the explicit rule documents the intent. (The first version of the probe wrongly required
  172.16.0.0/12; on the test PC the gateway is in 192.168.0.0/16. Fixed in the integration pass: any private or CGNAT gateway is accepted.)
- `iptables` is the first backend, with `nft` as a fallback (the brief's preference). Their script
  supported only iptables.
- The rules live in a new chain that is swapped in only when complete, so a failed run never leaves
  the walls half built.

## Checks run (no real WSL guest was used)

- `bash -n` on both scripts: pass. `shellcheck` is not installed on the PC, so it was not run.
- Stub tests of the apply script (stateful fake iptables, ip6tables, nft and ip; outside this folder):
  first apply builds 20 IPv4 and 7 IPv6 rules with DNS allowed only to the two resolvers; a second
  run leaves the chains identical; a failure in the middle of the rebuild leaves the old chain live;
  a leftover duplicate jump is removed; no default gateway fails with a reason; the nft fallback
  loads the expected rules; no backend fails; missing ip6tables with IPv6 present fails; non-root
  is refused without the test override.
- Probe checks: CIDR helper boundary cases pass; mirrored-networking, missing-status and
  open-port cases fail with the expected reasons; the full pass path passes when a timeout shim
  simulates dropped targets. On this PC, real TCP to Windows addresses is not a valid guest test.
- Test-only override: `PM_SBX_SKIP_ROOT_CHECK=1` lets the apply script run without root. Only the
  tests use it. The boot command runs as root.
