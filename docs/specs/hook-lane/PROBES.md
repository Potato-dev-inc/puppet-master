# Hook Lane probes (DESIGN-HOOK-LANE §9.1, step 0)

Run 2026-10-14/15 on the founder's PC (Windows 11). Claude Code 2.1.299 (auto-updated to 2.1.300
during the run), Codex CLI 0.162.1. Agents ran on a ConPTY with no window (scratch tool
`D:\pm-scratch-hook\tools\ptydrive`), with the host session's `CLAUDE*`/`ANTHROPIC*` env removed.
The hook command was a probe script (`capture.js`) that wrote stdin JSON plus the *names* of the
`PM_HOOK_*` env vars it could see.

**Token spend:** 4 real turns (Claude haiku: 2, Codex gpt-6-luna low: 2). Everything else was
zero-token (`-p --init-only`, startup screens, `/hooks`).

**Where:** Claude in `D:\pm-demo` (already trusted; no file written there except the turn's own
`hook-probe.txt`, removed afterwards; PM's hooks came in through `--settings`). Codex in a fresh
`D:\pm-scratch-hook\cx1` (the operator trusted that scratch folder and its probe hooks; the trust
records for that path are in `~/.codex/config.toml`, keyed by the scratch path only).

| Probe | Question | Result | Effect on v0 |
|---|---|---|---|
| P-A1 | Does a `--settings` `hooks` block merge with other settings levels or replace them? | **Merges.** `claude -p --init-only --settings <file>` in a folder whose `.claude/settings.local.json` also had a `SessionStart` hook: both hooks fired (same session id, 80 ms apart). | Claude install is not gated: ship `--settings`. |
| P-A2 | Do `async: true` hooks fire on `PermissionRequest` and `Stop`, and does the dialog still appear? | **Yes.** `PermissionRequest` arrived 0.2 s after `PreToolUse`, while the normal dialog was on screen; it stayed until answered. `Stop` arrived at the turn end with `last_assistant_message`. | async is right for every PM event. |
| P-A3 | Hooks before folder trust; `--init-only`. | **Interactive: nothing fires at all** (not even `SessionStart`) while the trust dialog is up; quitting at the dialog fires nothing. **`claude -p --init-only --settings f`** fires `SessionStart` + `SessionEnd` (source `startup`, reason `other`) with **no trust and no tokens**. `--init-only` is not in `--help` but works. | Doctor: "Waiting for you to trust this folder" when no `SessionStart` arrives; the zero-token wiring check is `-p --init-only`. |
| P-C1 | What does the Codex trust hash cover? | **The definition only.** Changing the bytes of the script the command runs: no re-review. Changing one hook's `command` string: "1 hook is new or changed" at the next start. | The frozen exe may be refreshed on update; the **command string** must stay byte-stable (golden test). |
| P-C2 | Is Codex hook trust per path? | **Yes.** Trust records are `[hooks.state.'<absolute hooks.json path>:<event>:<group idx>:<hook idx>'] trusted_hash = "sha256:..."`. A worktree at another path re-asks; the key is also **position**-sensitive. | PM appends its own matcher groups at the end of each event list and never reorders. Worktree runs re-ask (§10 Q3 stays open). |
| P-C3 | The review warning. | **A blocking startup screen**: "Hooks need review / N hooks are new or changed / 1. Review hooks 2. Trust all and continue 3. Continue without trusting (hooks won't run)". Fixtures: `tui/codex/fixtures/hooks_need_review*.txt`. | Added to the Codex `startup_gate` with **no dismiss**: a run parks instead of typing into it (typing "2" would trust all). |
| P-C4 | Does Codex expand `%VAR%` in a Windows hook `command`? | **No.** The `%PM_PROBE_TOOLS%` entry never ran. | The Codex command uses the literal per-user exe path (§10 Q9 answered for v0). |
| P-C5 | Do `PM_HOOK_*` env vars reach the hook process? | **Yes, both.** `PM_HOOK_TOKEN`, `PM_HOOK_PANE`, `PM_HOOK_BRIDGE` were visible to every Claude and Codex hook. | The env-token design works for both. |
| P-C6 | Codex `Stop` run **sync** with empty stdout. | **No error, no notice.** The sync `Stop` ran 24 ms after the async one. | Not needed in v0 (all async), but sync with empty stdout is safe. |

## Other facts found

- **Codex `SessionStart` is lazy:** it fires together with the first `UserPromptSubmit`, not when the
  TUI starts. Starting and quitting Codex without a prompt fires neither `SessionStart` nor
  `SessionEnd`. So "no `SessionStart` within 10 s of Ready" is **not** evidence of a trust problem
  for Codex; the doctor uses the trust screen and "has it ever reported" instead.
- **Codex Esc fires `Stop` *and* `Interrupt`** for the same `turn_id` (3 ms apart). The ledger
  treats an `Interrupt` after a `Stop` of the same turn as the turn's end reason.
- **Codex clamps `SessionEnd` and `Interrupt` timeouts to 3 s** and runs an async `SessionEnd`
  synchronously, with a warning in `/hooks`. PM's Codex scheme uses `timeout = 3` and does not
  subscribe `SessionEnd`, so no warning is printed for PM's entries.
- **Codex `PermissionRequest` for a patch** has `tool_name = "apply_patch"` and the whole patch in
  `tool_input.command` (sensitive: never persisted).
- **Claude `SubagentStop` after the top-level `Stop`** with the *same* `prompt_id` (a background
  helper agent, `agent_type: ""`). Subagent events never open or close turns.
- **Claude `PermissionRequest`** carries `permission_suggestions` and `tool_input` (`file_path`,
  `content`); `PostToolUse` carries `tool_use_id`, `duration_ms` and the full `tool_response`.
- **New common Claude fields:** `scratchpad_dir`, `permission_mode`, `effort.level`;
  `Stop` adds `background_tasks`, `session_crons`.

## Fixtures

`packages/app/src-tauri/src/agent_hooks/fixtures/<agent>/<cli version>/<VendorEvent>[-variant].json`,
the vendor payload only, with the Windows user name replaced by `user`. Prompts and replies in
them are the probe's own test text.

Missing variants (not captured, to save turns): Claude `StopFailure`, `UserPromptSubmit` with a
`<pasted_content>` wrapper, `Stop` with `stop_hook_active = true`.
