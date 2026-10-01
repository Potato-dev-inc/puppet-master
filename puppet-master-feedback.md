# Puppet Master hands-on feedback

Test date: October 1, 2026 (Asia/Taipei).

## Conversational use — latest feedback

The user asked me to work with Puppet Master as a collaborator, rather than run more synthetic tests. I used the existing OpenCode worker for a real read-only review of the current Rust MCP API. I asked which single small change would make an ordinary conversation easier, added a preference while it worked, and challenged its recommendation with a complementary source review from my native Luna low subagent.

### What the collaboration produced

OpenCode initially recommended clarifying `followup_task`'s description: the implementation already waits for up to 30 seconds and may return the settled answer, but the description does not say so. That can lead coordinators to make an unnecessary additional wait call.

Luna initially favored documenting background dispatch and result waiting. When challenged about the existing-worker handoff and the wait predicate, it revised its advice: current code can adopt a discovered `worker_id` directly, and an unfiltered wait may return a progress revision rather than a final answer.

I then told OpenCode that my actual loaded tools did not advertise `worker_id` on `run_agent` and did not expose `list_workers`. OpenCode revised its priority to synchronizing/refreshing the coordinator's catalog first, because the current source already implements direct adoption. It supported this with the tool definitions, native adoption path, route aliases, and stdio catalog-refresh warning. Its final recommendation was to make the existing direct-adoption capability visible before adding controls.

This was a useful exchange: both workers responded to substantive pushback instead of merely repeating their first proposal.

### How it felt compared with native Luna

Once attached, giving OpenCode a task, refining the preference, and discussing its answer felt natural. I could stay with one worker identity through the discussion. The worker carried the conversation forward and revised its recommendation.

The main difference was coordinator bookkeeping. My native Luna task took one follow-up call to dispatch, and its final answers arrived in the parent automatically. For Puppet Master I discovered the worker, took over its pane using the loaded contract, dispatched, and waited. The follow-up RPC then returned a bridge socket timeout even though the discussion turn completed; a later `wait_agents` call recovered the answer. That forced me to think about delivery state while I wanted to think about the proposal.

The initial OpenCode review completed in about 148 seconds and its discussion follow-up in about 44 seconds according to snapshots. These are whole-turn durations for different workloads/providers, not a model-speed benchmark. The usability issue is the uncertainty around an answer, not simply the number of seconds.

### Practical next step

Make the coordinator's served and loaded tool contract match the current implementation, then teach one happy path:

```text
list_agents → choose an adoptable worker_id
run_agent(worker_id, task) → stable handle and current result/state
followup_task(handle, next_message) → continue the conversation
wait_agents → only when the preceding call remains pending
```

This path was identified from the current code by both workers; direct `worker_id` adoption was not exercised through my older loaded schema during this conversation. Updating server definitions and refreshing a host's cached catalog are separate steps. A source file containing a field does not make it available to an already-connected coordinator.

Clarify bounded-wait semantics in `followup_task` and initialization instructions. When waiting for a final answer, use explicit terminal/input-required filters instead of teaching that every bare wait is a completion wait. Most importantly, a transport timeout after successful dispatch should retain the operation identity and give the coordinator a clear recovery path.

No new OpenCode pane was requested. I used the discovered existing pane `b8a0e14e-251b-4631-a613-f2c1288f1bf5`, returned manual control, and released its lease afterward. I did not inspect or claim process continuity in this conversational exercise. No repository files, settings, or MCP configuration were modified.

---

## Third trial — current assessment

The earlier Luna continuity failure no longer reproduced. Puppet Master now resumed the same Codex provider thread across ordinary follow-ups and retained the preceding user instruction, even though the preceding assistant answer contained only `ACK`.

| Check | Native Luna low | Puppet Master Luna low | Existing OpenCode worker |
|---|---|---|---|
| Remember ORBIT-83 while answering only ACK | ACK | ACK | ACK |
| Recall on plain follow-up | RETEST ORBIT-83 | RETEST ORBIT-83 | RETEST ORBIT-83 |
| Conversation identity preserved | Existing native agent reused | Same Codex thread ID across turns | Same native session and PID across tested tasks |
| Later reuse retained marker | Previously demonstrated | READY ORBIT-83 | Not additionally tested |

Puppet Master's Luna configuration was `gpt-6-luna` / `low`. The existing OpenCode worker used `opencode-go/deepseek-v4.1-flash`, so its timing is not a comparison of Luna model performance.

### Confirmed continuity improvement

Puppet Master's Codex thread ID remained `01a0f7ab-0310-76b2-bcf0-a3096e935072` for the ACK task, recall task, and later reuse. Both the typed transcript and snapshots supported real resume. Snapshots returned `context_continuity: resume`, `provider_session_id`, and `session_reset: false`.

I supplied no `context_mode` or `project_path` on the ordinary follow-up. This now behaves much more like using the native worker.

### Existing OpenCode process was preserved in this trial

I adopted the discovered pane `2d8cda8c-aea3-4ae6-be4d-4d10ed9101f9` rather than asking for a new OpenCode pane. Its PID stayed `8476`, its creation timestamp stayed unchanged, and its session ID stayed `ses_f0856afb5ffexmVx6qhQgtmgFS` through the ACK and recall tasks. The bridge reported no pending key swap.

This establishes ordinary reuse without replacement in this trial. Automatic key/profile recovery was not triggered, so the previous replacement/recovery problem is **not proven fixed**.

### Remaining priority 1: foreground follow-up can look failed after successful dispatch

The OpenCode recall call returned a recoverable `bridge_error` / socket timeout (`os error 10060`). Inspection afterward showed that the same turn had completed successfully with `RETEST ORBIT-83`, the same provider session, and `session_reset: false`.

The initial ACK task took approximately 60 seconds according to the worker snapshot. Provider response time may contribute to the delay; the defect from the coordinator's perspective is that the RPC error did not provide the already-dispatched operation state or explain how to retrieve its outcome.

Recommendation: return a handle/current snapshot before the transport deadline. On a foreground timeout, report an explicitly still-running or timed-out wait rather than losing the operation identity in a generic bridge error. Keep follow-up retry idempotent and offer one clear wait/recovery path.

Acceptance: delay a worker beyond the foreground wait budget; the caller still obtains the operation identity, can retrieve its eventual result, and does not risk dispatching a duplicate follow-up.

### Remaining priority 2: interruption result is not reliable on Windows

The Luna interruption call returned `WORKER_STOP_FAILED` with `taskkill could not stop the worker process tree`. The attempted long turn later had durable state `failed` / `WORKER_EXIT_FAILED`, not a confirmed interrupted result. A subsequent short task did succeed in the same Codex thread and returned `READY ORBIT-83`.

This is a successful reuse/memory check but **not a passing interruption check**. The transcript did not contain a final answer for the interrupted task. The recorded exit error used an Expo MCP authentication diagnostic, but successful turns also logged that diagnostic; it does not establish the underlying reason for exit.

Source inspection found that Windows termination suppresses taskkill stdout/stderr and may return an error after an unsuccessful exit status plus an immediate child-state check. That leaves too little diagnostic evidence to distinguish an actual inability to stop from a process-exit race.

Recommendation: preserve actionable termination diagnostics, reconcile child exit after the stop request, and publish an unambiguous terminal state. Only acknowledge interruption after confirming the intended outcome; retain the provider session for reuse.

Acceptance: interrupt an actively running Windows worker; it reaches a confirmed terminal state, leaves no unintended work running, and a follow-up resumes the same conversation with its marker intact.

### Remaining smaller API friction

- `list_agents` still returns a UI pane ID as `handle`, but `take_over(handle, grant=true)` rejects it as the wrong identity kind. Passing `pane_id` succeeds. Make discovered worker identities directly usable, or avoid labeling a pane-only identity as a run handle.
- The callable tool catalog still describes older delivery/status enums. I did not retest live OpenCode steering in this trial; schema freshness remains an earlier observation rather than a new steering result.

### Current verdict and cleanup

Normal delegation, memory, compact responses, and handle-only follow-ups are substantially closer to native subagents. Focus the next changes on trustworthy timeout/interruption outcomes and the final pane/handle mismatch. Maintain the newly working provider-session resume behavior.

The OpenCode pane was left open with manual control returned and my lease released. Its process/session remained unchanged through the final status check. I closed only my headless Codex test handle. The native Luna worker is idle. No Puppet Master source files were edited.

---

## Retest — updated findings

This section supersedes the first-run assessment below where behavior changed. The user requested a second hands-on trial after changes to the running application.

### Improvements confirmed

- `list_agents` now includes the existing UI-created OpenCode worker, with `worker_id`, working directory, backend, adoption/grant flags, and read-only capability. No mode switch was necessary to discover it.
- Adoption and dispatch worked without supplying `project_path`. Subsequent inspect, wait, and follow-up calls worked with the handle alone.
- Normal run/follow-up responses are smaller. A one-line memory response did not repeat the old analysis or capability notes. Full capability detail remained available through inspection.
- Puppet Master's Codex launch no longer failed on `--ask-for-approval`. A worker configured as `gpt-6-luna` / `low` completed the analysis from the Puppet Master repository.
- The OpenCode worker followed the three-item/word-budget format without an added evidence section. Its first memory follow-up returned `RETEST ORBIT-73` with resumed context.

### Current priority 1: headless Luna loses conversation context

I compared the native Luna worker against Puppet Master's headless worker configured as the same model and reasoning effort.

| Memory exercise | Native Luna low | Puppet Master Luna low |
|---|---|---|
| Recall initial ORBIT-73 marker | `RETEST ORBIT-73` | `RETEST [retention marker unavailable]` |
| Explicitly request resume afterward | Already retained | `RETEST UNKNOWN`; continuity reported `reconstructed_summary` |
| Packet policy: remember ORBIT-74 and reply only ACK, then recall | `RETEST ORBIT-74` | `RETEST [marker unavailable]` |

The ACK-only exercise matters: the secret to recall was present in the preceding user task but absent from the preceding assistant answer. Reconstructing context from only the last answer cannot preserve it. The transcript also showed different Codex thread IDs for the first task and follow-up. A stable Puppet Master handle therefore did not imply a stable Codex conversation.

Recommendation: resume the captured provider thread/session on ordinary follow-up. When real resume is unavailable, carry prior user instructions and relevant transcript context, not just the last result. An initial `fresh` launch should not silently make ordinary subsequent follow-ups fresh as well.

Acceptance: start a fresh Luna worker; tell it a marker while requiring an ACK-only answer; follow up using only the handle; it recalls the marker. Repeat under the default packet policy and after interruption.

### Current priority 2: same OpenCode pane ID concealed process/session replacement

I explicitly used the discovered existing pane `ebe5664a-2da2-49ea-a5c4-05a2330668a7`; I did not request an additional OpenCode pane. The first task and memory follow-up completed. During a later steering test, however:

- The pane PID changed from `49312` to `30992`, and its creation timestamp changed, despite its ID staying the same.
- The native session ID changed from `ses_f08a2130fffeEDKaZOMsMgNO04` to `ses_f08985f23ffeAaNk5XAtmnN03Q`.
- The bridge reported key-profile rotation from b to a, with `key_swap_pending: true`. Rotation is a possible explanation for the restart; causation was not established.
- The worker snapshot continued to reference the old provider session and reported `running` / `dispatched_waiting_for_task_completion`. The current native session's message list was empty.
- Steering returned `accepted`, but no final result arrived. I interrupted my test after approximately 206 seconds and released control once the user noticed the new OpenCode instance.

The registry contained one OpenCode pane before and after; that does not establish whether additional OS processes existed. OS-wide process enumeration was unavailable. What is confirmed is replacement of the process and native session associated with the reused pane.

Recommendation: make recovery/session replacement explicit in worker state; reconcile the handle's provider-session binding; preserve or replay task and conversation context during recovery; and resolve every accepted steering receipt. Reusing a worker must not silently substitute an empty conversation while continuing to claim that its earlier task is running.

Acceptance: trigger supported key/profile recovery during a task; the coordinator sees recovery state, the worker remains bound to the current native session, context survives, and the pending task either completes or reports a concrete failure rather than staying stale.

### Remaining smaller issues

- Discovery puts a UI pane ID in the `handle` field, but `take_over(handle, grant=true)` rejected it as a pane ID and required `pane_id` instead. Either keep these identities distinct in discovery or allow discovered worker identities consistently throughout adoption.
- Current callable MCP metadata still advertises `delivered|queued|deferred|unsupported`, while steering returned `accepted`; closing Codex handles still returned `closed` outside the advertised status enum. The runtime may have newer contracts than this host's loaded schemas. Treat host catalog refresh/version reporting as part of the fix.
- Projectless Codex launch failed with the now-actionable error `Not inside a trusted directory and --skip-git-repo-check was not specified`. Launching the same task in a repository succeeded. Support explicitly authorized projectless tasks while retaining the selected sandbox, or detect and explain this requirement before returning a running worker.

### Updated conclusion and cleanup

The ordinary OpenCode delegation path is much closer to native delegation now. The largest remaining difference is conversation continuity, especially across headless follow-ups and automatic native-worker recovery. Prioritize those behaviors over adding more orchestration controls.

I stopped the OpenCode steering test, returned manual control, and released its lease. The existing OpenCode pane was left open; no further tasks were dispatched to it after the user's correction. Both test Codex handles were closed. The native Luna worker is idle. No Puppet Master source files were edited.

---

## First trial — historical findings

Puppet Master's OpenCode lifecycle worked: delegate, steer, obtain a result, continue with memory, interrupt, and reuse. The biggest gap from native delegation was the work required to discover and attach the existing worker. Improve that path before adding more orchestration controls.

## What I actually tested

I spawned a native subagent configured as `gpt-6-luna` with `low` reasoning and fresh context. I also requested the same configuration through Puppet Master's Codex backend. That process failed before model inference. After the user pointed out an existing OpenCode worker, I adopted that worker and exercised its lifecycle. Its user-message metadata identified `opencode-go/mimo-v2.5`; its reasoning setting was unknown. This compares delegation workflows, not model quality or equal-model latency.

Both successful workers received the same local fixture and initial analysis prompt. The fixture included the marker `ORBIT-42` and the goal of making Puppet Master delegation feel as easy as native subagents. No Puppet Master source files were edited.

| Exercise | Native Luna low | Puppet Master / existing OpenCode |
|---|---|---|
| Start a small analysis | Completed after one spawn call | Completed after discovery, takeover, directory matching, and a fixture-read permission |
| Steering | Controlled BLUE → GREEN test returned `GREEN ORBIT-42` | Receipt advanced from `accepted` to `processed`; final analysis included the requested `MODEL CHECK` first item |
| Follow-up memory without rereading | Recalled marker and goal | Recalled marker and goal; reported `context_continuity: resume` |
| Interrupt an active turn | Returned prior status `running` | Returned `interrupted`, with an explicitly inferred interruption result |
| Reuse after interrupt | Returned `READY ORBIT-42` | Returned `READY ORBIT-42` on the same handle |
| Result collection | Final answers arrived in the parent automatically | Structured result obtained through `wait_agents` / follow-up response |
| Requested Luna through Puppet Master | — | Failed on an unsupported CLI argument before inference |

The native first task finished before my first steering message, so that attempt does not establish mid-turn delivery. I used a second, controlled waiting turn to verify native steering.

## Highest-priority changes

### 1. Make every existing worker discoverable in agent mode

Observed: `list_agents` omitted the existing UI-created OpenCode pane. Its documentation directs callers to `list_panes`, which was absent from this connection's agent-mode catalog. `set_mode(both)` reported 64 tools, but the available callable catalog did not gain the pane tools in this session. I had to read the repository's bridge documentation and use the documented local GET `/panes` route.

Change: expose a stable `list_workers` in agent mode that includes UI-created panes and managed runs. Each entry should provide a usable identity, backend, working directory, current state, model when known, and whether adoption requires a grant. Let `run_agent(worker_id, task)` adopt an eligible worker without switching tool catalogs.

Acceptance: from a fresh connection in agent mode, find and delegate to an existing idle OpenCode worker without shell mode, source inspection, or direct HTTP.

### 2. Bind the working directory to the worker identity

Observed: the pane's directory was `puppet-master/packages/app/src-tauri`. Supplying the repository root caused `PANE_UNAVAILABLE`, despite the native worker being healthy, attached, and having no pending permissions. Supplying its exact directory succeeded. Source inspection confirmed an exact normalized-directory equality check. Later, `take_over(handle)` without `project_path` reported that my owned handle was not registered; supplying the directory succeeded.

Change: resolve the worker's directory from its identity. Where an explicit directory conflicts, report `WORKSPACE_MISMATCH` with expected and supplied directories. A globally usable handle should route its own inspect, takeover, wait, and follow-up calls.

Acceptance: once adopted, all lifecycle calls work with the worker handle alone; a wrong explicit directory gives an actionable error.

### 3. Repair Codex launch compatibility and return the useful error

Observed: the requested Luna low worker exited after 94 ms. The typed transcript contained `unexpected argument '--ask-for-approval' found`. The top-level error only contained `For more information, try '--help'.`, requiring another call to discover the cause.

Installed CLI: `codex-cli 0.159.2`. `codex --help` lists `--ask-for-approval`; `codex exec --help` does not. The launch builder puts the flag after `exec`.

Source target: `packages/app/src-tauri/src/agent_adapters/launch.rs`, around lines 96–118. Move the top-level approval option before `exec`, preserving the existing sandbox policy. Validate the generated argument vector against supported CLI versions. This is a proposed fix, not an implemented or execution-tested patch.

Acceptance: a Puppet Master `gpt-6-luna` / `low` analysis produces a final response. Invalid CLI arguments surface their actual first diagnostic in `error.message`, including the CLI version and a concrete recovery hint.

### 4. Align tool schemas with actual runtime states

Observed: the advertised steering disposition enum was `delivered|queued|deferred|unsupported`. The working native backend returned `accepted` and subsequently `processed`. Closing my failed Codex worker returned `status: closed`, which was absent from the advertised status enum. A message sent after a rapidly failed run returned `queued`; a subsequent snapshot had no pending message, so the receipt did not explain its final disposition.

Change: generate MCP schemas from runtime enums, and explicitly represent accepted, processed, expired, and rejected delivery. Reject steering to a terminal turn with a clear suggestion to use `followup_task`, unless queuing for the next turn is an explicit supported behavior.

Acceptance: every observed status validates against the published schema; every message receipt has a queryable final disposition tied to a specific turn.

### 5. Return compact, current-turn responses by default

Observed: later memory and interruption responses carried the entire old analysis in `last_steer.result`, even while their current result was a short line. Full responses also repeated capability notes, IDs, verification fields, and both text and structured forms of the payload. This made successful calls harder to scan and consumed context unnecessarily.

Change: default lifecycle responses to identity, turn, status, result, prompt/error, and concise delivery state. Put old steering results and capability detail behind inspection/transcript tools or `detail: full`. Track cursors internally where possible. `wait_agents` already permits omitting cursors; retain that simple path rather than claiming cursors are required.

Acceptance: a one-line follow-up yields a compact response containing the current line, with no previous-turn answer copied into it.

## Other concrete observations

- The native OpenCode backend is the strongest tested path. It supported steering, session memory, structured results, interruption, and reuse. Preserve those behaviors.
- Permission handling worked with `allow_once` after I inspected the scope. The ordinary wait response only said `permission_required`; I needed a takeover screen to see the requested external-directory path. Include the action, resource, and scope in the structured prompt itself.
- `read_only: true` on the shared OpenCode pane returned `READ_ONLY_UNSUPPORTED`. I continued with an explicitly non-editing analysis instruction, not enforced read-only mode. Show this capability during discovery so callers can choose an isolated worker before launching.
- Completed OpenCode turns returned `verified: true` while `resolved_model` and `resolved_reasoning` remained null. This does not establish that verification is meant to cover the model. Split or label verification as result capture, model selection, and acceptance checks so callers can interpret it correctly.
- The rendered task repeated `Task:` wrappers and acceptance criteria, including instructions to return criteria evidence. The OpenCode answer consequently added extra scaffolding and exceeded the requested 350-word budget. Keep worker prompts faithful to the caller's output format and word budget; avoid adding unconditional evidence sections.
- Several bridge calls took roughly 4–8 seconds wall-clock even for short control operations. This is an observation from one session, not an isolated bridge benchmark. Instrument transport, bridge dispatch, provider startup, generation, and result reconciliation separately before optimizing.

## The default experience I would aim for

```text
worker = spawn_agent(task_name, message, model?, reasoning?, existing_worker?)
send_message(worker, correction)
result = wait_agent(worker)
followup_task(worker, next_task)
interrupt_agent(worker)
```

The coordinator should normally reason about one worker, its task, and its result. Project directories, pane identities, operation IDs, session attachment, and cursor bookkeeping should be resolved behind that API. Expose them for debugging and advanced control.

Do not reimplement memory or interruption first: both passed this exercise. First make discovery/adoption reliable, repair the Codex launch, and simplify the responses.

## Test cleanup and limits

I left the user's OpenCode pane open, returned manual control, and released my test worker leases. I closed the failed Codex test handle and restored this connection to agent mode. The native Luna worker is idle. No publishing, pushing, or source modification was performed.

This was a single-session usability exercise using a small analysis task and controlled lifecycle tests. Reconnection after process restart, concurrency contention, other providers, code editing, and numerical performance benchmarks were not tested.

Official background consulted: [OpenAI multi-agent documentation](https://developers.openai.com/api/docs/guides/responses-multi-agent). The findings above are based on the tool responses and local source/help inspected during this exercise, rather than assumed parity with that API.
