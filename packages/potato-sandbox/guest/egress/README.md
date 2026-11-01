# Sandbox network walls (guest egress)

These scripts stop the sandbox from reaching the Windows PC and the local network.
The sandbox can still use the internet.

The sandbox runs in its own WSL2 distro (Ubuntu 24.04). The walls are firewall rules
inside that distro. They are set at every boot.

## What is blocked and what is allowed

Blocked (no connection can be made):

- The Windows host. That is the WSL default gateway, and the DNS tunnel address `10.255.255.254`.
- Private and special IPv4 ranges: `0/8`, `10/8`, `100.64/10`, `127/8` (except the local loopback),
  `169.254/16` (includes cloud metadata), `172.16/12`, `192.0.0/24`, `192.168/16`,
  `198.18/15`, multicast `224/4`, and reserved `240/4`.
- Private IPv6: unique-local `fc00::/7`, link-local `fe80::/10`, and multicast `ff00::/8`.
  The one exception is the link-local ICMPv6 that the network needs to work (`ff02::/16`).

Allowed:

- Everything on the public internet.
- Loopback (`lo`) and replies to connections that were already open.
- DNS (port 53, UDP and TCP) to the servers listed in `/etc/resolv.conf`. This still works
  when a DNS server has a private address, such as `10.255.255.254`. Every other port on
  those addresses is blocked.

## Files

| File | What it does |
|---|---|
| `pm-sbx-egress.sh` | Sets the walls. Runs as root at boot. Writes `/run/pm-sbx/egress.status`. |
| `pm-sbx-egress-probe.sh` | Checks that the walls really work. Run it after the walls. Exit 0 means pass. |
| `README.md` | This file. |
| `PORTED.md` | Which parts came from the source project and what was changed. |

## Install

Copy both scripts to the distro, for example to `/usr/local/sbin/`, and make them readable by root.
The base recipe installs both to `/usr/local/sbin/`. WSL runs them at boot through the one boot command in
`/etc/wsl.conf`, which is `/usr/local/sbin/pm-sbx-boot.sh` (see `guest/desktop/`). That script runs the WSLg cut, then
`pm-sbx-egress.sh`, then `pm-sbx-egress-probe.sh`, and writes `/run/pm-sbx/boot.done` only when all three pass.

Restart the distro after editing `wsl.conf` (`wsl --terminate <distro>` from Windows, then start it again).
Never use `wsl --shutdown`: it stops every other distro on the PC too.

Needs `iptables` (on Ubuntu 24.04 this is `iptables-nft`) or `nft`, plus `ip`, `awk` and `timeout`.
`curl` is used for the internet check. Without it, the probe checks a plain TCP connection instead.

## Check it

```bash
sudo cat /run/pm-sbx/egress.status          # state=applied means the walls are set
bash /usr/local/sbin/pm-sbx-egress-probe.sh  # PASS means the walls hold and the internet works
cat /run/pm-sbx/egress-probe.status          # the last probe result, with a reason if it failed
```

Anything that starts sandbox work should read `state=passed` in `/run/pm-sbx/egress-probe.status`
first, and refuse to start if it is missing or says `failed`.

## If the probe fails

| Reason (in the status file) | What it means | What to do |
|---|---|---|
| `egress rules not applied` | The walls did not get set at boot. | Read `/run/pm-sbx/egress.status`, fix the `reason=` line, run the apply script again. |
| `mirrored networking not supported: ...` | WSL is in mirrored networking mode, or the network does not look like the WSL NAT setup. | Switch the distro to the default NAT networking. The walls cannot be trusted in mirrored mode. |
| `walls not working: TCP to ... connected` | Something accepted a connection that should be blocked. | Check the chains: `sudo iptables -S PM_SBX_EGRESS`. Run the apply script again. |
| `walls not working: TCP to ... quick error` | A connection was refused or failed at once, not dropped. | The walls should drop, not refuse. Check the rules. |
| `internet: https://example.com did not answer` | The internet check failed. | Check DNS (`/etc/resolv.conf`) and the connection to the internet. |

## Limits

- Mirrored networking is not supported. The probe fails on purpose.
- The probe accepts a WSL NAT gateway in any private range (`10/8`, `172.16/12`, `192.168/16` or CGNAT `100.64/10`). A gateway outside those, or none at all, fails closed.
- The walls only cover the distro's own traffic. Anything running as root inside the distro can change them.
  The `pm` user has no sudo, so it cannot.
- Public DNS and the public internet stay open, so a program could still tunnel data out through them.
- IPv6 addresses that wrap private IPv4 space (NAT64 `64:ff9b::/96`, 6to4 `2002::/16`) are not blocked.
- The probe checks a few common ports and the metadata address. It is a smoke test, not a full audit.
- The rules are cleared at each boot. The boot command in `wsl.conf` must run.

Test on a real WSL2 distro before you rely on these walls. The scripts were checked on a
development PC with stub firewall commands only. No real WSL2 distro was used.
