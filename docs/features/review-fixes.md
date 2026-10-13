# Review fixes (branch `feat/review-fixes`)

Source: `D:\pm-night\REVIEW-personas.md`, section 3 (top 10 fixes), 2.1 (attention model), 2.8 (accessibility).
Cut from `integration/night` f0e4760. No MCP catalog change. Backup branch: `backup/feat-review-fixes-0654`.

## What and why

Personas A (developer), B (vibecoder), C (non-technical). Each item closes a gap the persona review found.
Everything stays agent-agnostic: no agent-name matching anywhere; risk comes from `prompt_risk`, identity from the
pane registry.

## Item -> change -> evidence

| # | Item | Change | Evidence |
|---|---|---|---|
| 1 | Lock-screen Allow gated by risk (SECURITY) | `push/triggers.rs`: `PromptInfo.risk`; `actionable` also needs a known, non-high risk (missing/unknown fails closed). Body = `High risk · <headline>` newline command. Pane title leads the notification title (`auth refactor · Claude Code needs you`). `push/hub.rs` re-checks risk before attaching `act`. | Rust tests `a_high_risk_command_never_gets_lock_screen_buttons`, `a_missing_or_unreadable_risk_fails_closed`, `low_and_medium_risk_commands_are_actionable`, `the_body_leads_with_the_risk_sentence_then_the_command`, `the_pane_title_leads_the_notification_title`, hub `a_high_risk_or_unrated_command_is_open_only_but_says_the_risk`. Failing before: `only_command_approvals_with_allow_once_and_deny_are_actionable` failed ("assertion failed: info.actionable") as soon as risk became required, until its fixture got a `risk`. `cargo test --lib push::` 53 passed. |
| 4 | ask_user choices | `QuestionCard.tsx`, `mobile/QuestionsList.tsx`: all choices secondary (no `i === 0` primary). Caption "Claude Code stops waiting in 9 min" from `expires_at_ms` (`lib/questions.ts` `waitCaption`; the phone re-renders every 30 s). | `QuestionCard.test.tsx`, `QuestionsList.test.tsx`: no primary class, caption text and edge wording. Screenshot `cockpit-questions.png`. |
| 9 (part) | Undo safety | New `RestoreSheet` shared by History and the new `UndoSheet`; palette "Undo last turn…" opens it (with shared-folder warning) instead of restoring silently. `restore-flow.ts` `restoreWithUndo`: every restore toast carries **Undo**, which restores `safetyId` ("Before undo"). `notify(text, action?)` in `useCockpit`; toasts keep 12 s when they have an action. | `UndoSheet.test.tsx` (last-turn pick, confirm required, nothing-to-undo), `Palette.undo.test.tsx` (never calls `checkpointUndoLast`), `History.test.tsx` (Undo restores `4`). Screenshot `review-fixes-restore-toast.png`. |
| 8 | `.puppet-master/` out of git | New `git_exclude.rs`: appends `.puppet-master/` to the repo `info/exclude` (follows a worktree/submodule `.git` file and `commondir`, creates a missing `info/`, reads `.gitignore` but never writes it, idempotent, no git process, nothing for non-git folders, cached per project). Called before first write by checkpoints (`checkpoints/git.rs`), worktrees, operations store, turn summaries, event log, message receipts. | 9 Rust tests in `git_exclude::tests`: untracked `.puppet-master` visible before / `git status` clean after, idempotent, subfolder, linked worktree writes to the main repo, anchored pattern only at root, non-repo untouched. `cargo test --lib` 1492 passed. |
| 3 | One attention model | `lib/attention.ts` `deriveNeedsYou` (approval, question, error, stuck, answer; `gate` reserved). `hooks/useNeedsYou.ts`: CockpitApp derives once (incl. Ask-bar notices) and shares via context. Inbox hero Doto + Inbox badge + sidebar "Needs you" caption count all use `items.length`; errored panes are Inbox cards ("PowerShell hit an error · Open · Ask another agent"); stuck panes join the Needs-you group. Phone: `waitingCount(panes, {questions, stuckPaneIds})` is the same derivation; `AgentsOverview` fetches questions itself (`questionsRevision`) so its counter includes them. | `lib/attention.test.ts` (6), `attention.surfaces.test.tsx`: fixture mix of 2 approvals + 1 error + 1 stuck + 1 question gives `05` / `5` / `5` on Doto, badge and group, and `05` on the phone. Screenshot `cockpit-inbox.png`. |
| 6 | Identity on titled rows | `titles.ts` `paneSubline` / `multiLoginAgents`: second line `Codex · Editing src/app.ts`, plus `· Work` when that agent has panes under more than one login; errored rows read `Error · project`; palette uses the same helper. | `titles.test.ts`, `Titles.dom.test.tsx`. Screenshot `cockpit-titles.png`. |
| 7 (part) | Review before Merge | `Changes` takes a `secondOpinion` slot rendered between the file list and Merge/Discard (PaneView passes `SecondOpinion`; without a worktree it renders alone). `reviewHasConcerns` -> Merge becomes secondary "Merge anyway". `reconcileVerdictText`: a count in the reviewer's sentence that disagrees with the notes ("Two things" vs 4 notes) is replaced by ours ("4 notes below."). | `Changes.review.test.tsx` (order, no-worktree, Merge anyway, clean keeps primary), `review-diff.test.ts`. Screenshot `second-opinion-changes.png`. |
| 8 (a11y) | `--label-3` contrast | dark `0.4 -> 0.52`; light `0.42/0.5 -> 0.64` (label-2 light `0.64 -> 0.74` to stay louder). Four token files: cockpit, phone, askbar, settings. | `styles/label-contrast.test.ts` computes blended contrast: dark 5.31 / 5.30 / 5.21 (on #0a0a0b / #111112 / #18181a); light 5.07 / 4.90 (on #fff / #f5f5f7), settings light 4.75 (on #ececef). Before: dark 3.56-3.61, light 2.57-2.61. |

## Verification (final, on the branch head)

- `npx tsc --noEmit -p packages/app`: no output, rc 0
- `npx vitest run` in packages/app: 111 files, 969 passed (base 920)
- `cargo test --lib`: 1492 passed, 0 failed, 2 ignored (base 1476)
- `cargo test --bins`: 33 passed, 0 failed
- Screenshots (headless harness, looked at): `D:\pm-night\evidence\review-fixes\` `cockpit-inbox.png` (error card, 2 = Doto = badge = group), `cockpit-questions.png` (equal-weight choices, countdown), `second-opinion-changes.png` (review above "Merge anyway"), `review-fixes-restore-toast.png` (toast with Undo), `cockpit-titles.png` (identity line), `cockpit-palette.png`.
  The restore-toast scenario parks the 3-12 s timers because the screenshot is taken seconds after the scenario ends.

## Not done (honest)

- **Fix #2, phone approval card.** The phone bridge client (`lib/bridge.ts`) has no way to read the pane's current prompt or answer it (no `/agents/answer` client method, no prompt GET); that is a bridge-route plus client plus card job, not S. Left for a dedicated branch. `sw.ts` open intent untouched.
- Fix #7 remainder (Merge keeps the pane; soft Discard), Fix #9 remainder (History grouped by turn, "Tell the agent I undid this", large-file notice), Fix #5, #10.
- Tray glyph / taskbar badge and the Away digest still count on their own (Rust-side digest); only the sidebar, Inbox and phone counter share the model.
- A "one-time notice" for the `.puppet-master/` exclusion is not shown. Trade-off to decide: `.puppet-master/memory.md` and `agents/*.toml` are shareable by design but are now ignored locally (already-tracked files are unaffected, `git add -f` still works).
- Risk word "High risk" is still label-3 small text (contrast now passes; weight change from 2.8 not done).

## Overlaps (file level) with other branches

`push/*`, `QuestionCard.tsx`, `mobile/QuestionsList.tsx`, `Palette.tsx`, `History.tsx`, `Changes.tsx`, `SecondOpinion.tsx`, `Sidebar.tsx`, `Inbox.tsx`, `CockpitApp.tsx`, `PaneView.tsx`, `useCockpit.ts` (notify signature), `AgentsOverview.tsx`, `PwaApp.tsx`, and the four token CSS files. All merged tonight on `integration/night`; this branch is a follow-up on top.
