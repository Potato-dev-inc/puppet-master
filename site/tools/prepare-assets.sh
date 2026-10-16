#!/usr/bin/env bash
# Rebuilds site/assets/*.webp from the real screenshots (cropped to the part that matters).
# Needs ffmpeg with libwebp. Sources live outside the repo, so this script is the record of where each shot came from.
# Privacy: every crop below was chosen to leave out the pairing QR code, the pairing code and the tunnel URL
# (Settings > Phone is cropped below them), the project path on Settings > Bridge & MCP, and any home path that is
# not the made-up "dev" user. Rejected on purpose (see the notes at the bottom): shots that show a tunnel URL,
# a QR code or a pairing code, a real-looking user folder, or the old npx config.
# Usage: FFMPEG=/c/ffmpeg/bin/ffmpeg bash site/tools/prepare-assets.sh
set -euo pipefail
FFMPEG="${FFMPEG:-ffmpeg}"
OUT="$(cd "$(dirname "$0")/.." && pwd)/assets"

# Sources
C=/d/pm-night/p0/cowork-v1/shots                # real two-profile Co-work run (maya and leo, two apps on one PC)
L=/d/pm-night/v021-live/shots                   # 0.2.1 real runs with real agents
M=/d/pm-night/p0/missions/live/shots            # 0.2.1 real Missions run
PH=/d/pm-night/p0/phone-approval/shots/after    # 0.2.1 phone approval card (390 px, dev harness)
S=/d/pm-night/ux-polish/v021/shots3             # 0.2.1 UI from the dev harness (real UI code, example data)
SW=/d/pm-night/release/v0.2.1/swtest            # 0.2.1 first-run welcome from the real build
R=/d/pm-night/real-shots-021                    # 0.2.1 installed app, real agents, 144 shots (shots.md lists each)
mkdir -p "$OUT"

# name | source | w:h:x:y   (lossless, used for the older real-run shots that stay as they are)
shot() {
  local name="$1" src="$2" crop="$3"
  "$FFMPEG" -v error -y -i "$src" -vf "crop=${crop}" -c:v libwebp -lossless 1 -compression_level 6 "$OUT/$name.webp"
  echo "ok $name"
}
# name | source | w:h:x:y   (lossy quality 80, at most 1600 px wide)
shotq() {
  local name="$1" src="$2" crop="$3"
  "$FFMPEG" -v error -y -i "$src" -vf "crop=${crop},scale='min(1600,iw)':-2" -c:v libwebp -quality 80 -compression_level 6 "$OUT/$name.webp"
  echo "ok $name"
}

# ---- Real runs, 0.2.1 (Claude Code and Codex; pane names are random ids) ----
shotq hero-desktop        "$L/B1-approval-card.png"          1280:820:0:0
shotq approval            "$L/B1-approval-card.png"          700:365:410:30
shotq trust               "$L/A3-codex-trust-card-words.png" 700:365:410:30

# ---- Real two-profile Co-work run (maya and leo, 0.2.0 UI), kept as is: steps 2 and 4 have no 0.2.1 equivalent ----
shot team-toast       "$C/live-maya-4-toast.png"      700:145:410:212
shot team-queue       "$C/live-leo-4-queue-sheet.png" 410:340:555:240
shot team-result      "$C/live-leo-6-result.png"      700:320:410:418
shot team-applied     "$C/live-leo-8-applied.png"     700:260:410:418

# ---- 0.2.1 harness: cockpit, Inbox, approvals ----
shotq approval-high    "$S/approval-high.png"          700:345:490:30
shotq trust-declined   "$S/cockpit-inbox-error.png"    700:350:490:118
shotq inbox-mixed      "$S/inbox-mixed.png"            700:880:490:20
shotq inbox-consent    "$S/inbox-consent.png"          700:620:490:20
shotq install-sheet    "$S/doctor-install-sheet.png"   700:360:490:260

# ---- Receipts, undo ----
shotq receipt-details  "$S/cockpit-receipt-details.png"  740:420:250:455    # Folder row shows the made-up dev user
shotq history-toobig   "$S/cockpit-history-too-big.png"  700:110:250:150

# ---- Separate copies, merge, second opinion ----
shotq second-opinion   "$S/second-opinion-changes.png"   1030:640:240:225
shotq second-picker    "$S/second-opinion-picker.png"    520:300:580:300

# ---- Budgets ----

# ---- Agents that start agents (Settings > Bridge & MCP) ----

# ---- Missions (Preview) ----
shotq mission-tamper   "$S/mission-tamper-card.png"      700:420:490:470

# ---- Simple mode (Labs) ----

# ---- First run (the Documents row, which shows a user folder name, is cropped out of the project step) ----

# ---- Tour ----
shotq digest           "$S/cockpit-digest.png"           700:520:490:40
shotq askbar           "$S/askbar-answer.png"            680:470:380:15
shotq handoff          "$S/handoff-inbox-card.png"       700:355:490:235

# ---- Security and phone settings (Settings > Phone is cropped above the public address and below nothing sensitive) ----

# ---- Phone (390 px wide, the app's own UI on example data) ----
shotq phone-low        "$PH/phone-approval-low.png"               390:844:0:0
shotq phone-high       "$PH/phone-approval-details.png"           390:844:0:0
shotq phone-watch-only "$PH/phone-approval-watch-only.png"        390:844:0:0
shotq phone-trust      "$PH/phone-trust-gate.png"                 390:844:0:0
shotq phone-team       "$S/phone-team.png"                        390:844:0:0

# ---- Co-work, 0.2.1 UI (example data: Maya, Leo, Sam) ----
shotq team-note        "$S/cockpit-team-toast.png"               940:405:490:480
shotq team-github      "$S/cockpit-team-github.png"              700:230:490:635   # starts below the first card, which names a made-up third teammate
shotq team-settings    "$S/settings-team.png"                    700:330:490:190   # ends above the join requests

# ---- Real 0.2.1 shots (installed app, real agents; D:/pm-night/real-shots-021, see shots.md there) ----
# These replace the harness "Example data" assets of the same name. Crop is w:h:x:y on the 1440x900 (phone: 780x1688) source.
# Not used on purpose: shots with a logged defect (014, 018, 019, 072, 140-144) and 034/052 (Codex sandbox failure on that PC).
shotq cockpit-inbox    "$R/043-cockpit-working-needs-you-idle.png" 1440:900:0:0
shotq cockpit-light    "$R/138-light-theme-inbox.png" 1440:900:0:0
shotq palette          "$R/022-command-palette.png" 600:410:420:120
shotq cockpit          "$R/038-approval-low-risk-terminal.png" 1440:900:0:0
shotq setup-agents     "$R/124-set-up-agents.png" 620:560:526:30
shotq changes          "$R/062-changes-file-diff.png" 1190:540:250:95
shotq merge-dialog     "$R/065-merge-sheet.png" 420:190:640:355
shotq newagent-copy    "$R/016-new-agent-menu.png" 300:390:155:42
shotq project-copies   "$R/128-project-copies.png" 620:520:490:15
shotq receipt          "$R/046-receipt-after-check-first.png" 620:230:250:100
shotq history          "$R/055-history-checkpoints.png" 1190:200:250:90
shotq history-restore  "$R/056-undo-sheet.png" 400:230:640:335
shotq history-toast    "$R/057-undo-done-history.png" 360:70:540:825
shotq budget-chip      "$R/032-trust-codex-terminal.png" 1190:210:250:0
shotq budget-card      "$R/070-inbox-budget-card.png" 700:150:487:210
shotq budget-settings  "$R/067-settings-budgets.png" 700:180:580:65
shotq memory           "$R/127-project.png" 700:620:490:15
shotq fr-welcome       "$R/001-welcome-1-how-do-you-work.png" 640:520:400:0
shotq fr-agents        "$R/005-welcome-2-both-ready.png" 640:470:400:335
shotq fr-project       "$R/007-welcome-3-project.png" 640:440:400:60
shotq fr-task          "$R/010-welcome-4-result.png" 640:710:400:60
shotq phone-agents     "$R/132-phone-home.png" 780:1688:0:0
shotq phone-terminal   "$R/134-phone-claude-idle-terminal.png" 780:1688:0:0
shotq phone-devices    "$R/122-settings-phone-paired.png" 700:228:590:640
shotq security         "$R/103-settings-general-p1.png" 700:400:590:497
shotq labs             "$R/105-settings-general-p3.png" 700:300:590:345
shotq toolbelt         "$R/115-settings-bridge-mcp-p1.png" 700:500:590:75
shotq pack-preview     "$R/118-settings-add-puppet-master-preview.png" 680:780:380:62
shotq mission-sheet    "$R/076-mission-form-filled.png" 580:594:430:157
shotq mission-live     "$R/081-mission-tab.png" 1190:480:250:95
shotq simple-preview   "$R/098-simple-preview-done.png" 1440:900:0:0
shotq simple-home      "$R/089-simple-your-projects.png" 760:300:340:0
shotq simple-see       "$R/094-simple-review-prompt.png" 760:440:340:20
shotq simple-ask       "$R/036-simple-working.png" 1440:200:0:0

# Rejected for privacy (not used): first-run-phone.png, settings-mobile.png, settings-phone-pairing*.png and
# settings-phone-pair-minted.png (tunnel URL), settings-team-invite.png (QR code and pairing code),
# settings-bridge-more.png and settings-bridge-install-result.png (the old npx @puppet-master/mcp config),
# phone-menu.png and phone-confirm.png (bridge address), phone-approval-answered-elsewhere@390.png (a named iPhone),
# first-run-project*.png below the Recent list's Documents row (a user folder name).
