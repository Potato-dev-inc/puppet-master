// Live regression scenarios (run_agent / steer / cancel / timeout / reconnect ...) against the
// running app. Each scenario takes (backend, extra) and records assertions through ctx.check.
import { PONG } from './context.mjs';

const SLOW_TASK = 'Run the shell command `python -c "import time; time.sleep(60)"` and then reply DONE.';
const STEER_TASK = 'Run the shell command `node -e "setTimeout(()=>{},20000)"` and wait for it to finish, then reply with only the word PONG.';

export function createScenarios(ctx) {
  const { tool, check, sandbox, handlesSeen } = ctx;

  /** Run to a terminal state, following watch/wait when the first call returns still running. */
  async function runToEnd(runArgs) {
    let view = await tool('run_agent', { project_path: sandbox, wait_ms: 90_000, ...runArgs });
    if (view.handle) handlesSeen.push(view.handle);
    for (let i = 0; view.status === 'running' && i < 12 && view.handle; i += 1) {
      view = await tool('wait_agents', { handles: [view.handle], timeout_ms: 60_000, project_path: sandbox });
      view = view.runs?.[0] ?? view.agents?.[0] ?? view;
    }
    return view;
  }
  
  /** A visible Cursor pane in the suite's own fresh sandbox stops on the first-run workspace-trust
   *  dialog. Trusting that folder (created by this script, empty) is part of the scenario; anything
   *  else that blocks is left for the assertions to report. */
  async function resolveSandboxTrust(view) {
    for (let i = 0; i < 6 && view.status === 'needs_input' && view.prompt?.kind === 'workspace_trust' && view.handle; i += 1) {
      await tool('answer_prompt', { handle: view.handle, prompt_id: view.prompt.id, choice: 'trust', project_path: sandbox });
      view = await tool('wait_agents', { handles: [view.handle], timeout_ms: 90_000, project_path: sandbox });
      view = view.runs?.[0] ?? view.agents?.[0] ?? view;
    }
    return view;
  }

  const scenarios = {
    async pong(backend, extra) {
      const view = await runToEnd({ agent_type: backend, task: PONG, checks: [{ type: 'result_contains', text: 'PONG' }], ...extra });
      check('pong: completed', view.status === 'completed', `${view.status} ${view.error?.code ?? ''}`);
      check('pong: acceptance passed', view.acceptance_status === 'passed', view.acceptance_status);
      check('pong: result contains PONG', /PONG/.test(view.result ?? ''), String(view.result).slice(0, 80));
      return view;
    },
    async write(backend, extra) {
      const view = await runToEnd({
        agent_type: backend,
        task: 'Create a file named hello.txt in the current folder containing exactly the text REGRESSION-OK. Do nothing else.',
        checks: [{ type: 'file_contains', path: 'hello.txt', text: 'REGRESSION-OK' }],
        ...extra,
      });
      check('write: completed', view.status === 'completed', `${view.status} ${view.error?.code ?? ''}`);
      check('write: file check passed', view.acceptance_status === 'passed', view.acceptance_status);
      return view;
    },
    async failing_check(backend, extra) {
      const view = await runToEnd({ agent_type: backend, task: PONG, checks: [{ type: 'result_contains', text: 'ZZZ-NOT-PRESENT' }], ...extra });
      check('failing_check: ACCEPTANCE_FAILED', view.error?.code === 'ACCEPTANCE_FAILED' || view.acceptance_status === 'failed', `${view.status} ${view.error?.code ?? ''}`);
      check('failing_check: never verified', view.verified !== true || view.acceptance_status !== 'failed');
      return view;
    },
    // command_exits: exit code matches (default 0 and an explicit non-zero), and a mismatch fails.
    async command_exits(backend, extra) {
      const node = process.execPath;
      const pass = await runToEnd({ agent_type: backend, task: PONG, checks: [
        { type: 'command_exits', command: [node, '-e', 'process.exit(0)'] },
        { type: 'command_exits', command: [node, '-e', 'process.exit(3)'], expect_exit: 3 },
      ], ...extra });
      check('command_exits: matching exit codes pass', pass.acceptance_status === 'passed', `${pass.status} ${pass.acceptance_status} ${pass.error?.code ?? ''}`);
      const fail = await runToEnd({ agent_type: backend, task: PONG, checks: [
        { type: 'command_exits', command: [node, '-e', 'process.exit(1)'] },
      ], ...extra });
      check('command_exits: mismatch fails acceptance', fail.error?.code === 'ACCEPTANCE_FAILED' || fail.acceptance_status === 'failed', `${fail.status} ${fail.acceptance_status}`);
      check('command_exits: failing run is never verified', fail.verified !== true);
      return fail;
    },
    // Headless Cursor cannot run shell without allow_shell. If it says so, the run must carry the
    // warning and must not be verified; a rejected call must never read as a clean success.
    async shell_rejected(backend, extra) {
      const view = await runToEnd({ agent_type: backend, task: 'Using your shell tool, run `node --version` and report the output.', ...extra });
      const said = /rejected|not in allowlist|unable to run|could not run|cannot run/i.test(view.result ?? '');
      if (said) {
        check('shell_rejected: SHELL_COMMAND_REJECTED warning present', (view.warnings ?? []).some((w) => w.code === 'SHELL_COMMAND_REJECTED'), JSON.stringify(view.warnings));
        check('shell_rejected: not verified', view.verified !== true);
      }
      return view;
    },
    // Steering: opencode takes live steers and the receipt must reach `processed`; a backend without
    // live steering must say so up front and never claim `accepted`.
    async steer(backend, extra) {
      const started = await tool('run_agent', { agent_type: backend, project_path: sandbox, task: STEER_TASK, background: true, wait_ms: 0, ...extra });
      if (started.handle) handlesSeen.push(started.handle);
      if (!check('steer: run started', started.handle, started._error)) return started;
      // A steer sent while the pane is still spawning has no session to land in and is (correctly)
      // queued then expired, so wait until the prompt has been dispatched, as a real caller would.
      for (let i = 0; i < 60; i += 1) {
        const peek = await tool('inspect_agent', { handle: started.handle, project_path: sandbox });
        if (peek.stage === 'dispatched_waiting_for_task_completion' || ['completed', 'failed'].includes(peek.status)) break;
        await new Promise((resolve) => setTimeout(resolve, 500));
      }
      const receipt = await tool('send_message', { handle: started.handle, message: 'Correction: reply with only the word STEERED instead of PONG.', project_path: sandbox });
      const live = backend === 'opencode_native';
      if (live) check('steer: live backend accepts', ['accepted', 'processed'].includes(receipt.disposition), receipt.disposition ?? receipt._error);
      else check('steer: non-live backend never claims accepted', receipt.disposition !== 'accepted', receipt.disposition);
      let view = started;
      for (let i = 0; i < 10 && !['completed', 'failed', 'cancelled'].includes(view.status); i += 1) {
        const waited = await tool('wait_agents', { handles: [started.handle], timeout_ms: 60_000, project_path: sandbox });
        view = waited.agents?.[0] ?? view;
      }
      const inspected = await tool('inspect_agent', { handle: started.handle, project_path: sandbox });
      check('steer: inspect reports supports_live_steer', inspected.supports_live_steer === live, String(inspected.supports_live_steer));
      if (live) {
        check('steer: result follows the correction', /STEERED/.test(view.result ?? ''), String(view.result).slice(0, 60));
        check('steer: receipt reaches processed', view.last_steer?.state === 'processed' || inspected.last_steer?.state === 'processed', JSON.stringify(inspected.last_steer));
      }
      return view;
    },
    // Steering immediately after run_agent, before the pane has even opened: the message must be
    // delivered live once the prompt dispatches, not queued and lost.
    async steer_early(backend, extra) {
      if (backend !== 'opencode_native') return {};
      const started = await tool('run_agent', { agent_type: backend, project_path: sandbox, task: STEER_TASK, background: true, wait_ms: 0, ...extra });
      if (started.handle) handlesSeen.push(started.handle);
      if (!check('steer_early: run started', started.handle, started._error)) return started;
      const receipt = await tool('send_message', { handle: started.handle, message: 'Correction: reply with only the word STEERED instead of PONG.', project_path: sandbox });
      check('steer_early: delivered live, not queued', ['accepted', 'processed'].includes(receipt.disposition), receipt.disposition ?? receipt._error);
      let view = started;
      for (let i = 0; i < 10 && !['completed', 'failed', 'cancelled'].includes(view.status); i += 1) {
        const waited = await tool('wait_agents', { handles: [started.handle], timeout_ms: 60_000, project_path: sandbox });
        view = waited.agents?.[0] ?? view;
      }
      check('steer_early: result follows the correction', /STEERED/.test(view.result ?? ''), String(view.result).slice(0, 60));
      return view;
    },
    // Visible pane, two turns, then close_agent must close the pane (B1) and the first turn must
    // never report success without the prompt having landed.
    async visible_close(backend) {
      const first = await resolveSandboxTrust(await runToEnd({ agent_type: backend, headless: false, keep_pane: true, task: PONG }));
      const ok = check('visible: first turn completed with PONG', first.status === 'completed' && /PONG/.test(first.result ?? ''), `${first.status} ${first.error?.code ?? ''} ${String(first.result).slice(0, 60)}`);
      const paneId = first.pane_id;
      if (ok && first.handle) {
        let second = await tool('followup_task', { handle: first.handle, task: 'Reply with only the word PING. Use no tools.', wait_ms: 90_000, project_path: sandbox });
        for (let i = 0; second.status === 'running' && i < 12; i += 1) {
          second = await tool('wait_agents', { handles: [first.handle], timeout_ms: 60_000, project_path: sandbox });
          second = second.runs?.[0] ?? second.agents?.[0] ?? second;
        }
        check('visible: follow-up completed with PING', second.status === 'completed' && /PING/.test(second.result ?? ''), `${second.status} ${second.error?.code ?? ''}`);
      }
      if (first.handle) await tool('close_agent', { handle: first.handle, project_path: sandbox });
      if (paneId) {
        const panes = await tool('list_panes', {});
        const alive = JSON.stringify(panes).includes(paneId);
        check('visible: pane gone after close_agent', !alive, paneId);
      }
      return first;
    },
  
    // ---- recovery: duplicate prevention, cancellation, timeouts, reconnect ----------------------
  
    /** The same idempotency_key must never start a second execution. */
    async duplicate(backend, extra) {
      const key = `dup-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
      const start = () => tool('run_agent', { agent_type: backend, task: PONG, project_path: sandbox, idempotency_key: key, background: true, wait_ms: 0, ...extra });
      const first = await start();
      const second = await start();
      if (first.handle) handlesSeen.push(first.handle);
      check('duplicate: both calls return the same handle', Boolean(first.handle) && first.handle === second.handle, `${first.handle} vs ${second.handle}`);
      check('duplicate: both calls return the same operation', Boolean(first.operation_id) && first.operation_id === second.operation_id, `${first.operation_id} vs ${second.operation_id}`);
      let view = first;
      for (let i = 0; i < 12 && !['completed', 'failed', 'cancelled'].includes(view.status) && first.handle; i += 1) {
        view = await tool('wait_agents', { handles: [first.handle], timeout_ms: 30_000, project_path: sandbox });
        view = view.runs?.[0] ?? view.agents?.[0] ?? view;
      }
      check('duplicate: the single run completed', view.status === 'completed', view.status);
      const turns = JSON.stringify(await tool('inspect_agent', { handle: first.handle, project_path: sandbox }));
      check('duplicate: only one turn exists', !turns.includes('"turn_index":1') && !turns.includes('"turn-1"'), 'a second turn was created');
    },
  
    /** A multi-line task (three lines plus acceptance criteria) must reach the worker. It used to
     *  be classified as terminal input for opencode_native and typed into the TUI, where it was lost
     *  (PROMPT_NOT_SUBMITTED after 45 s). */
    async multiline(backend, extra) {
      const view = await runToEnd({
        agent_type: backend,
        task: 'Do these two things in order.\n1. Create a file named multi.txt containing exactly MULTI-OK.\n2. Reply with only the word DONE.',
        acceptance_criteria: ['Reply with only the word DONE', 'multi.txt exists'],
        checks: [{ type: 'file_contains', path: 'multi.txt', text: 'MULTI-OK' }, { type: 'result_contains', text: 'DONE' }],
        ...extra,
      });
      check('multiline: completed', view.status === 'completed', `${view.status} ${view.error?.code ?? ''}`);
      check('multiline: checks passed', view.receipt?.outcome === 'passed_checks', String(view.receipt?.outcome));
    },
  
    /** The same replay guarantee when a worker name, checks, locks and criteria are in the request,
     *  and a changed request under the same key must still conflict. */
    async duplicate_named(backend, extra) {
      const key = `dupn-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
      const name = `worker-${Math.random().toString(16).slice(2, 8)}`;
      const body = { agent_type: backend, task: PONG, project_path: sandbox, idempotency_key: key, name, checks: [{ type: 'result_contains', text: 'PONG' }], locks: [`file:dupn-${key}.txt`], acceptance_criteria: ['be brief'], background: true, wait_ms: 0, ...extra };
      const first = await tool('run_agent', body);
      const second = await tool('run_agent', body);
      if (first.handle) handlesSeen.push(first.handle);
      check('duplicate_named: identical retry returns the same handle', Boolean(first.handle) && first.handle === second.handle, `${first.handle} vs ${second.handle} ${second._error ?? ''}`);
      check('duplicate_named: identical retry returns the same operation', Boolean(first.operation_id) && first.operation_id === second.operation_id, `${first.operation_id} vs ${second.operation_id}`);
      const changed = await tool('run_agent', { ...body, task: 'Reply with only the word DIFFERENT. Use no tools.' });
      check('duplicate_named: a changed request under the same key conflicts', changed.code === 'IDEMPOTENCY_KEY_CONFLICT' || /idempotency/i.test(String(changed._error)), String(changed.code ?? changed._error ?? changed.status));
      let view = first;
      for (let i = 0; i < 12 && !['completed', 'failed', 'cancelled', 'interrupted'].includes(view.status) && first.handle; i += 1) {
        view = await tool('wait_agents', { handles: [first.handle], timeout_ms: 30_000, project_path: sandbox });
        view = view.runs?.[0] ?? view.agents?.[0] ?? view;
      }
      check('duplicate_named: the single run completed', view.status === 'completed', view.status);
    },
  
    /** cancel_agent must end a running turn as cancelled, never as a success. */
    async cancel(backend, extra) {
      const view = await tool('run_agent', { agent_type: backend, project_path: sandbox, task: SLOW_TASK, background: true, wait_ms: 0, ...(backend === 'cursor_agent' ? { allow_shell: true } : {}), ...extra });
      if (view.handle) handlesSeen.push(view.handle);
      if (!check('cancel: run started', Boolean(view.handle), JSON.stringify(view).slice(0, 160))) return;
      await new Promise((resolve) => setTimeout(resolve, 9_000));
      await tool('cancel_agent', { handle: view.handle, project_path: sandbox });
      let final = view;
      // A cancelled turn reads `interrupted` (cancel_agent/interrupt_agent) or `cancelled`.
      for (let i = 0; i < 30; i += 1) {
        final = await tool('inspect_agent', { handle: view.handle, project_path: sandbox });
        if (['completed', 'failed', 'cancelled', 'interrupted'].includes(final.status)) break;
        await new Promise((resolve) => setTimeout(resolve, 1_000));
      }
      check('cancel: ends cancelled or interrupted', ['cancelled', 'interrupted'].includes(final.status), final.status);
      check('cancel: never reported as success', final.status !== 'completed' && final.receipt?.outcome !== 'passed_checks', `${final.status} ${final.receipt?.outcome}`);
      check('cancel: receipt says cancelled', final.receipt?.outcome === 'cancelled', String(final.receipt?.outcome));
      // keep_pane defaults to false, so cancel_agent releases the pane the run created.
      await new Promise((resolve) => setTimeout(resolve, 2_500));
      const livePanes = JSON.stringify(await tool('list_panes', {}));
      check('cancel: the run-spawned pane is released', !view.pane_id || !livePanes.includes(view.pane_id), String(view.pane_id));
    },
  
    /** A run that exceeds timeout_ms fails with a recoverable TIMEOUT and stays inspectable. */
    async timeout(backend, extra) {
      const view = await tool('run_agent', { agent_type: backend, project_path: sandbox, task: SLOW_TASK, timeout_ms: 12_000, background: true, wait_ms: 0, ...(backend === 'cursor_agent' ? { allow_shell: true } : {}), ...extra });
      if (view.handle) handlesSeen.push(view.handle);
      if (!check('timeout: run started', Boolean(view.handle), JSON.stringify(view).slice(0, 160))) return;
      let final = view;
      for (let i = 0; i < 45; i += 1) {
        final = await tool('inspect_agent', { handle: view.handle, project_path: sandbox });
        if (['completed', 'failed', 'cancelled', 'interrupted', 'timeout'].includes(final.status)) break;
        await new Promise((resolve) => setTimeout(resolve, 2_000));
      }
      check('timeout: run did not complete', final.status !== 'completed', final.status);
      check('timeout: status is timeout', final.status === 'timeout', final.status);
      check('timeout: error code is TIMEOUT', final.error?.code === 'TIMEOUT', JSON.stringify(final.error)?.slice(0, 160));
      check('timeout: marked recoverable', final.error?.recoverable === true, String(final.error?.recoverable));
      check('timeout: receipt outcome is failed', final.receipt?.outcome === 'failed', String(final.receipt?.outcome));
    },
  
    /** A second connection cannot touch another connection's worker until it attaches with the
     *  owner's token; after attaching it can close it. Uses the bridge directly (two sessions). */
    async reconnect(backend, extra) {
      if (backend !== 'opencode_native') return; // fast backend is enough to prove ownership recovery
      let base = null;
      for (let port = 17321; port <= 17335 && !base; port += 1) {
        try {
          const health = await (await fetch(`http://127.0.0.1:${port}/health`, { signal: AbortSignal.timeout(500) })).json();
          if (health.ok) base = `http://127.0.0.1:${port}`;
        } catch { /* next port */ }
      }
      if (!check('reconnect: bridge found', Boolean(base))) return;
      const stamp = `${Date.now()}`;
      const a = `recov-A-${stamp}`;
      const b = `recov-B-${stamp}`;
      const call = async (session, method, path, body) => {
        const response = await fetch(`${base}${path}`, {
          method,
          headers: { 'Content-Type': 'application/json', 'X-Puppet-Master-Session': session },
          body: body === undefined ? undefined : JSON.stringify(body),
          signal: AbortSignal.timeout(120_000),
        });
        const text = await response.text();
        let json = {};
        try { json = JSON.parse(text); } catch { json = { raw: text }; }
        return { status: response.status, ...json };
      };
      await call(a, 'POST', '/mcp/mode', { mode: 'agent' });
      await call(b, 'POST', '/mcp/mode', { mode: 'agent' });
      const run = await call(a, 'POST', '/agents/run', { task: PONG, agent_type: backend, project_path: sandbox, keep_pane: true, wait_ms: 60_000, idempotency_key: `recov-${stamp}` });
      if (!check('reconnect: owner run completed', run.status === 'completed', `${run.status} ${run.code ?? ''}`)) return;
      const identity = await call(a, 'GET', '/mcp/session');
      check('reconnect: owner has an attach token', Boolean(identity.attach_token), JSON.stringify(identity).slice(0, 120));
      const denied = await call(b, 'POST', '/agents/close', { handle: run.handle, project_path: sandbox });
      check('reconnect: a stranger cannot close the worker', denied.code === 'AUTHORIZATION_DENIED', `${denied.status} ${denied.code}`);
      const attached = await call(b, 'POST', '/agents/attach', { attach_token: identity.attach_token, coordinator_name: 'recovery-test' });
      check('reconnect: attaching with the owner token succeeds', attached.status === 200 || attached.ok === true || Array.isArray(attached.attached) || Boolean(attached.attached), JSON.stringify(attached).slice(0, 200));
      const closed = await call(b, 'POST', '/agents/close', { handle: run.handle, project_path: sandbox });
      check('reconnect: the new connection can close it after attaching', closed.status === 'closed' || closed.stage === 'closed', `${closed.status} ${closed.code ?? ''}`);
      const panes = JSON.stringify(await tool('list_panes', {}));
      check('reconnect: the pane is gone after close', !run.pane_id || !panes.includes(run.pane_id), String(run.pane_id));
    },
  };

  return scenarios;
}
