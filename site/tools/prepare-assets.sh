#!/usr/bin/env bash
# Rebuilds site/assets/*.webp from the real screenshots (lossless WebP, cropped to the part that matters).
# Needs ffmpeg with libwebp. Sources live outside the repo, so this script is the record of where each shot came from.
# Privacy: every crop below was chosen to leave out the pairing QR code, the pairing code and the tunnel URL
# (Settings > Phone is cropped below them) and the project path on Settings > Bridge & MCP.
# Usage: FFMPEG=/c/ffmpeg/bin/ffmpeg bash site/tools/prepare-assets.sh
set -euo pipefail
FFMPEG="${FFMPEG:-ffmpeg}"
OUT="$(cd "$(dirname "$0")/.." && pwd)/assets"
V=/d/pm-night/verify-r10/shots
C=/d/pm-night/p0/cowork-v1/shots
mkdir -p "$OUT"

# name | source | w:h:x:y
shot() {
  local name="$1" src="$2" crop="$3"
  "$FFMPEG" -v error -y -i "$src" -vf "crop=${crop}" -c:v libwebp -lossless 1 -compression_level 6 "$OUT/$name.webp"
  echo "ok $name"
}

# Real runs (Claude Code and Codex on a Windows PC, r10 live verification)
shot hero-desktop     "$V/edit-card-1.png"      1280:820:0:0
shot setup-agents     "$V/33-setup-agents.png"  540:570:490:40
shot approval         "$V/edit-card-1.png"      700:365:410:30
shot trust            "$V/t2b-1-card.png"       700:365:410:30
shot merge            "$V/100-wt1-merge-sheet.png" 1040:420:240:95
shot receipt          "$V/t1-receipt.png"       740:345:250:220
shot history          "$V/wt1-history.png"      1040:175:240:95
shot security         "$V/40-security.png"      700:305:505:296
shot phone-devices    "$V/50-phone-devices.png" 690:435:505:355

# Real two-profile Co-work run (maya and leo, two apps on one PC)
shot team-toast       "$C/live-maya-4-toast.png"     700:145:410:212
shot team-queue       "$C/live-leo-4-queue-sheet.png" 410:340:555:240
shot team-result      "$C/live-leo-6-result.png"     700:320:410:418
shot team-applied     "$C/live-leo-8-applied.png"    700:260:410:418

# Harness screenshots (the real UI code on example data; the page says "Example data" next to them)
shot phone-agents     "$C/phone-agents.png"      390:844:0:0
shot phone-team       "$C/phone-team.png"        390:844:0:0
shot team-settings    "$C/settings-team.png"     700:580:490:120
