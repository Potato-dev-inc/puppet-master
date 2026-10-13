# Second opinion: ask another agent to review what this one just did

Branch `feat/second-opinion` (cut from `integration/night` `18c3d0f`). Also covers idea 20, "comment to follow-up review".

## 1. What and why

One action in the cockpit's Receipt tab: **Second opinion**. PM takes the diff of what the agent just did, hands it to a
*different* agent in a read-only run, and shows the answer as notes pinned to the diff lines plus a one-line verdict.
Every note, and any free comment on a diff line, can be sent back to the agent that wrote the code with one click.

Cross-vendor review (for example Codex reviewing Claude's change) is something a vanilla agent app cannot do, because
each one only talks to its own model.

- **A developers:** a cheap second pair of eyes before merging a worktree; a GitHub-PR-style loop (review, comment,
  agent fixes) locally and instantly.
- **B vibecoders:** "does this look right?" without reading a diff. The verdict line says "Looks good", "Some concerns" or
  "Changes needed" in plain words.
- **Agent-agnostic:** the reviewer is *any* agent whose profile declares read-only headless support
  (`Features.read_only_headless`, via `tui::read_only_backends()`). The author can be any agent or plain pane. Nothing
  in the code matches an agent name; the answer format is a plain text contract that every model can follow, with a
  tolerant parser and a JSON fallback.

## 2. Design

### Flow
```
Receipt tab  "Second opinion"  ->  sheet: pick a reviewer (headless-capable, installed; "ready" = warm worker)
   -> second_opinion_start(pane, reviewer)
        1. pick the diff            (source.rs)       worktree  >  last-turn checkpoint  >  git diff HEAD
        2. build the prompt         (prompt.rs)       task + turn summary + receipt facts + project memory + diff + format
        3. run_agent(read_only)     (service.rs)      headless, background, no shell, no trust grant, fresh context
   -> UI polls second_opinion_poll(project, handle) every 2 s (DotSpinner while it runs; Stop cancels)
   -> parse_review(answer)          (parse.rs)        lines, then JSON, then plain text
   -> notes pinned to diff lines + verdict
Note / comment  "Send to <agent>"  ->  second_opinion_send(pane, file, line, text)   (route.rs)
        pane has a run handle  ->  followup on that handle (same path as `followup_task`: next turn, or queued)
        hand-opened pane       ->  typed into the pane and submitted (same path as `write_pane_input`)
```

### Files (under `packages/app/src-tauri/src`)
| Path | Role |
|---|---|
| `second_opinion/mod.rs` | shared types (`SourceKind`, `ReviewDiff`, `Finding`, `Verdict`, `ParsedReview`) |
| `second_opinion/source.rs` | `Sources` trait + `select` (worktree, then checkpoint, then git), the 48 KiB cap (cut at a file boundary), counting, the capped `git diff HEAD` |
| `second_opinion/prompt.rs` | prompt assembly and the answer-format rules; the diff fence is longer than any backtick run inside the diff |
| `second_opinion/parse.rs` | tolerant findings parser, verdict reader, JSON fallback, plain-text fallback; caps (100 findings, 600 chars each) |
| `second_opinion/route.rs` | follow-up routing (`Targets` / `Sender` traits) and the one-line message |
| `second_opinion/service.rs` | live wiring: reviewers list, `build_run_request`, `start`, `poll`, `cancel`, `send_to_author` |
| `commands/second_opinion.rs` | Tauri commands `second_opinion_agents`, `_start`, `_poll`, `_cancel`, `_send` |
| `checkpoints/engine.rs`, `runtime.rs` | new `last_turn_patch` (unified diff of the newest "after" against the checkpoint the turn started from) |

Frontend (`packages/app/src`): `lib/second-opinion.ts` (types + Tauri calls, its own file so the shared `cockpit-api.ts`
is untouched), `components/cockpit/SecondOpinion.tsx` (button, picker sheet, spinner, verdict, notes),
`AnnotatedDiff.tsx` (diff lines with a gutter marker, the note inline below its line, a per-line comment box),
`review-diff.ts` (line numbering from hunk headers, path matching, anchoring), `second-opinion-store.ts` (runs per pane
outside React so a review survives tab switches), `styles/cockpit-second-opinion.css`. `Changes.tsx` shows the same
annotated diff when a worktree review exists.

### Decisions
- **Diff source.** A worktree wins because it is the whole of what the run changed. Otherwise the last turn's checkpoint
  diff ("before turn n" against "after turn n"; a turn whose "before" was skipped because nothing had changed starts at the
  previous "after"). Last resort: `git diff HEAD` of the pane's folder (tracked files only; untracked files are not in it).
  The UI says which one was used ("Codex reviewed the last turn"). The patch is cut at 48 KiB on a file boundary and the
  prompt and UI both say so.
- **Read-only is enforced, not requested.** `build_run_request` only accepts agents in `read_only_backends()` (it reuses
  `quick::select_backend`, the same gate as `quick_ask`), sets `read_only: true`, `allow_shell: false`,
  `trust_workspace: false`, headless, fresh context; `run_agent` then refuses a read-only run it cannot enforce. The
  reviewer's run is keyed on the origin project, never on the worktree folder.
- **Answer format.** `FILE:LINE | severity | finding` lines plus `VERDICT: approve|concerns|changes | sentence`, with a
  JSON object accepted as the fallback. The parser takes bullets, numbering, tables, bold, backticks, `file#L12`,
  `file:12-15` and Windows paths, ignores lines it cannot read, and keeps unparseable answers as plain text.
- **Monochrome UI.** Severity is weight plus glyph (`!!` high, `!` medium, `·` low, `–` note); no colour.
- **Staleness.** The review is of a snapshot; the panel shows "the agent is working again, so these notes may be out of
  date" when the pane starts running again.

### OSS
None needed. (`@git-diff-view/react` / CodeMirror merge from idea 20 were not adopted: the existing monochrome diff viewer
plus 200 lines of anchoring was smaller than the dependency.)

## 3. Evidence

Run in `D:\pm-second-opinion`, `CARGO_BUILD_JOBS=3`. Nothing was launched (no app, no browser window, no real agent run).

```
cargo test --lib            (src-tauri)  -> test result: ok. 1459 passed; 0 failed; 2 ignored
cargo test --lib second_opinion          -> test result: ok. 26 passed; 0 failed
cargo test --lib last_turn_patch         -> checkpoints::tests::last_turn_patch_is_the_newest_after_against_where_that_turn_started ... ok
npm test --workspace=@puppet-master/app  -> Test Files 104 passed (104); Tests 901 passed (901)     (base was 77 / 578)
npx tsc --noEmit -p packages/app         -> exit 0, no output
```

### Rust tests (`second_opinion/tests.rs`, 26, plus 1 in `checkpoints/tests.rs`)
- **Diff source selection:** `the_worktree_diff_wins_and_later_sources_are_not_even_asked`,
  `the_checkpoint_diff_is_used_when_there_is_no_worktree_diff` (both "not isolated" and "isolated but empty" fall through),
  `plain_git_is_the_last_resort_and_nothing_at_all_is_an_error`, `a_big_patch_is_cut_at_a_file_boundary_and_marked`,
  `the_selected_diff_is_counted`, `count_ignores_header_lines_and_binary_files`,
  `git_diff_head_reads_uncommitted_changes_and_is_none_outside_a_repo` (real `git`, temp repo);
  `checkpoints::tests::last_turn_patch_is_the_newest_after_against_where_that_turn_started` (real shadow repo: a running
  turn gives none; a turn whose "before" was skipped still diffs only its own change; the cap cuts and says so).
- **Prompt assembly:** `the_prompt_carries_task_facts_context_diff_and_the_answer_format`,
  `the_prompt_works_with_nothing_but_a_diff_and_says_when_it_was_cut`,
  `a_worktree_diff_tells_the_reviewer_the_files_on_disk_may_not_show_it`,
  `the_diff_fence_cannot_be_closed_by_the_diff_itself`, `a_long_task_is_clipped_so_the_diff_still_fits`.
- **Findings parsing:** `well_formed_lines_and_a_verdict`, `agents_that_drift_from_the_format_are_still_read` (bullets,
  numbering, table rows, bold, backticks, `#L30`, Windows path), `a_json_block_is_the_fallback` (fenced object, bare array,
  string verdict), `garbage_is_tolerated_and_kept_as_plain_text`, `findings_none_with_an_approve_verdict_is_a_clean_review`,
  `findings_are_capped_and_long_text_is_cut`.
- **Read-only enforcement:** `the_review_run_is_headless_read_only_without_a_shell_or_trust`,
  `an_agent_that_cannot_enforce_read_only_is_refused_as_reviewer` (opencode, a shell, nonsense and an empty name are
  refused; a missing install is `BACKEND_UNAVAILABLE`),
  `every_offered_reviewer_is_a_read_only_backend_and_the_authors_agent_is_marked`.
- **Follow-up routing:** `a_pane_with_a_run_handle_gets_a_followup_on_that_handle`,
  `a_hand_opened_pane_gets_the_comment_typed_into_it`, `a_closed_pane_empty_text_and_a_dead_pane_are_clear_errors`,
  `a_comment_is_one_plain_line_with_file_and_line`. Poll mapping: `polling_maps_a_run_to_running_done_or_failed`.

### Fails before, passes after
`an_agent_that_cannot_enforce_read_only_is_refused_as_reviewer` caught a real bug while writing the feature:
`quick::select_backend` treats an empty hint as "no hint" and silently picks the first installed agent, so an empty reviewer
id would have started a review with an agent nobody chose. With the guard removed from `build_run_request`:
```
test second_opinion::tests::an_agent_that_cannot_enforce_read_only_is_refused_as_reviewer ... FAILED
called `Result::unwrap_err()` on an `Ok` value: AgentRunRequest { ..., agent_type: "claude", headless: true, ... read_only: true ...
test result: FAILED. 0 passed; 1 failed
```
With the guard (`INVALID_ARGUMENT: Pick an agent to review.`): `... ok`. (All other new tests cannot run on `18c3d0f` at all:
the module and `last_turn_patch` do not exist there.)

### Frontend tests (new)
`review-diff.test.ts` (line numbering from hunks, path matching, anchoring), `AnnotatedDiff.test.tsx` (note under its line,
gutter marker, Send to X, the per-line comment box sends `file:line`, no send buttons when the author is unknown),
`second-opinion-store.test.ts` (7: phases, failure text, cancel, late answers ignored, send errors),
`SecondOpinion.test.tsx` (13: **picker** lists headless-capable agents, hides the author's own by default, "ready"
preselected, "Also allow" adds it, Start review passes the chosen id; spinner while running and Stop; verdict and meta;
**inline notes**; **Send to agent**; unanchored notes list; worktree notes only pointed to Changes; failure; unstructured
answer), `Changes.review.test.tsx` (5: **worktree notes are pinned inside Changes**: first noted file auto-opens, the note
follows its line, count and gutter marker, Send to agent; a checkpoint review leaves Changes plain; no review = the old
plain `<pre>`).

### HTTP routes
None: the feature uses Tauri commands only (`second_opinion_agents|start|poll|cancel|send`) and no MCP catalog change
(`catalog_version_is_pinned` untouched).

### Screenshots (`D:\pm-night\evidence\second-opinion`, headless harness, opened and checked)
- `second-opinion-picker.png`: "Who should review?" with Codex (ready, preselected), Cursor Agent, and the unchecked
  "Also allow Claude Code, the agent that wrote this".
- `second-opinion-changes.png`: Changes with `session.ts` open: a `!` marker in the gutter and the note under line 13, `!!`
  and the note under line 15, each with "Send to Claude Code"; "2 notes" on the file row, "1 note" on `token.ts`; below, the
  panel's verdict line ("Some concerns. Two things to fix before merging.") and one unanchored note under "Elsewhere".
  Command: `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\second-opinion second-opinion-picker second-opinion-changes`
  (the script in this tree already has the free-port and per-run-token check, so it ran without the extra fix hash).

## 4. Overlap with other branches (file level)

New files (no overlap): `second_opinion/*`, `commands/second_opinion.rs`, `lib/second-opinion.ts`,
`components/cockpit/{SecondOpinion,AnnotatedDiff,review-diff,second-opinion-store}.ts(x)` and their tests,
`styles/cockpit-second-opinion.css`, `dev/scenarios/second-opinion-*.ts`.

Edits to existing files (merge hot-spots, all small):

| File | Change | Likely overlap |
|---|---|---|
| `src-tauri/src/lib.rs` | `mod second_opinion;` and 5 handlers after `commands::worktree_diff` | any branch registering commands |
| `src-tauri/src/commands/mod.rs` | `mod` + `pub use` line | same |
| `src-tauri/src/checkpoints/engine.rs`, `runtime.rs`, `tests.rs` | new `last_turn_patch`, `TurnPatch`, `Runtime::last_turn_patch` and a test (append-only) | `feat/checkpoints`, `feat/away-digest`, anything extending checkpoints |
| `src-tauri/src/agent_runs.rs` | one line `pub(crate) use runtime::{cancel_agent, send_agent};` | run-lifecycle branches |
| `src-tauri/src/agent_runs/pane_api.rs` | `run_of_pane` made `pub(crate)` | none expected |
| `src/components/cockpit/Changes.tsx` | notes pinned via `AnnotatedDiff` when a worktree review exists; `reviewApi` prop; note count on file rows | `feat/worktree-isolation`, any diff-viewer work |
| `src/components/cockpit/PaneView.tsx` | one import and one `<SecondOpinion .../>` line in the Receipt tab | every branch touching the Receipt tab |
| `src/dev/fixtures.ts`, `src/dev/mock-tauri.ts` | appended fixtures and five `second_opinion_*` cases | other scenario authors (append-only) |

## 5. Limits and follow-ups

- **No live end-to-end run.** The rules forbid launching the app, so the path `second_opinion_start` -> real `run_agent` ->
  `poll` -> answer was not exercised with a real reviewer CLI. Everything on both sides of it is tested (request shape,
  read-only gate, poll mapping from a recorded snapshot, parser), and it reuses the `quick_ask` / Ask bar path. First thing
  to try by hand: pick Codex to review a Claude pane.
- **Reviewer answer quality is unverified.** The format is a contract in the prompt; models drift, which is why the parser
  accepts many shapes and keeps unparseable answers as plain text.
- **`git diff HEAD` skips untracked files.** New files an agent created outside a worktree show only through the other two
  sources. Checkpoints off plus an untracked-only change gives "nothing to review".
- **Notes pin to new-file line numbers.** A note on a deleted line, or on a line outside any hunk, is listed under "Elsewhere"
  instead of inline.
- **Snapshot, not live.** The review is of the diff at that time; if the agent keeps working, the panel says the notes may be
  out of date. A worktree review is pinned in Changes against the live diff, so a note whose line moved falls out of the inline
  view (it is still in the review). Re-run to refresh.
- **Worktree reviews read the origin folder.** The reviewer's run is keyed on the origin project (a merge or discard deletes
  the worktree), so files on disk may not show the change; the prompt says to trust the diff.
- **Context pack:** the coordination `build_context_pack` is task and lock oriented, not useful for review, so the prompt
  carries the shared project memory (`one_brain`), the turn sentence and the receipt facts instead.
- **Send to author** for a pane with a run handle starts the next turn (or queues behind a running one) through `send_agent`;
  a hand-opened pane gets the line typed and submitted. A run-backed pane the user has since typed into by hand is still
  treated as a run. A closed pane gives `AGENT_GONE` ("nobody to send this to").
- **Checkpoint turn numbers:** an existing quirk (a skipped "before" leaves the next "after" with the previous turn number)
  is worked around by `last_turn_patch` using adjacency, not turn numbers. `last_turn_start` (Undo last turn) still has it.
- Polling every 2 s (no push event). Phone: none tonight. Cancel is best effort.
