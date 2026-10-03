#!/usr/bin/env node
// Live regression suite for Puppet Master agent runs. Unlike test-mcp-operations.mjs (mock
// bridge), this drives the real MCP binary against the RUNNING app and real backends, so it
// spawns agents and costs a little. Repeat runs catch the intermittent bugs a single pass hides.
//
//   node scripts/pm-regression.mjs [--backends opencode_native,cursor_agent] [--repeat 3]
//        [--only pong,write,failing_check,visible_close] [--binary path] [--project dir] [--keep]
//
// Exit code 1 on any failed assertion, a verified:true result that is wrong, a leftover pane, or
// a handle left running. Cursor runs use trust_workspace on the suite's own temp folder only.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, index, all) => {
    if (arg.startsWith('--')) pairs.push([arg.slice(2), all[index + 1]?.startsWith('--') || all[index + 1] === undefined ? true : all[index + 1]]);
    return pairs;
  }, []),
);
const backends = String(args.backends ?? 'opencode_native,cursor_agent').split(',');
const repeat = Number(args.repeat ?? 1);
const only = args.only ? String(args.only).split(',') : null;
const binary = String(args.binary ?? join(root, 'packages', 'mcp-server', 'dist', process.platform === 'win32' ? 'puppet-master-mcp.exe' : 'puppet-master-mcp'));
if (!existsSync(binary)) {
  console.error(`MCP binary not found: ${binary}`);
  process.exit(2);
}

// --project reuses an existing folder (e.g. one Cursor already trusts, for visible-pane runs); it is never deleted.
const ownSandbox = !args.project;
const sandbox = ownSandbox ? mkdtempSync(join(tmpdir(), 'pm-regression-')) : String(args.project);
const proc = spawn(binary, [], { stdio: ['pipe', 'pipe', 'inherit'], windowsHide: true });
const pending = new Map();
let buffer = '';
let nextId = 1;
proc.stdout.on('data', (chunk) => {
  buffer += chunk;
  for (let newline = buffer.indexOf('\n'); newline >= 0; newline = buffer.indexOf('\n')) {
    const line = buffer.slice(0, newline).trim();
    buffer = buffer.slice(newline + 1);
    if (!line) continue;
    try {
      const message = JSON.parse(line);
      pending.get(message.id)?.(message);
    } catch { /* notification or noise */ }
  }
});

function rpc(method, params, timeoutMs = 600_000) {
  const id = nextId++;
  proc.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`timeout: ${method}`)); }, timeoutMs);
    pending.set(id, (message) => { clearTimeout(timer); pending.delete(id); resolve(message); });
  });
}

async function tool(name, toolArgs) {
  const response = await rpc('tools/call', { name, arguments: toolArgs });
  const result = response.result;
  if (!result) return { _error: response.error?.message ?? 'no result' };
  const payload = result.structuredContent ?? (() => { try { return JSON.parse(result.content?.[0]?.text ?? '{}'); } catch { return { raw: result.content?.[0]?.text }; } })();
  return result.isError ? { _error: payload?.message ?? payload?.raw ?? 'tool error', ...payload } : payload;
}

const results = [];
const handlesSeen = [];
const failures = [];
function check(label, condition, detail = '') {
  if (!condition) failures.push(`${label}${detail ? `: ${detail}` : ''}`);
  return Boolean(condition);
}

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

const PONG = 'Reply with only the word PONG. Use no tools.';
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
    const slow = `Run the shell command \`node -e "setTimeout(()=>{},20000)"\` and wait for it to finish, then reply with only the word PONG.`;
    const started = await tool('run_agent', { agent_type: backend, project_path: sandbox, task: slow, background: true, wait_ms: 0, ...extra });
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
    const slow = `Run the shell command \`node -e "setTimeout(()=>{},20000)"\` and wait for it to finish, then reply with only the word PONG.`;
    const started = await tool('run_agent', { agent_type: backend, project_path: sandbox, task: slow, background: true, wait_ms: 0, ...extra });
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
    const first = await runToEnd({ agent_type: backend, headless: false, keep_pane: true, task: PONG });
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
};

async function main() {
  await rpc('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'pm-regression', version: '1' } });
  proc.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
  const health = await tool('bridge_health', {});
  if (health._error) {
    console.error(`bridge unreachable (is the Puppet Master app running?): ${health._error}`);
    process.exit(2);
  }
  console.log(`build ${health.build_id}  catalog ${health.catalog_version}  drift=${health.schema_drift ?? 'n/a'}`);
  check('health: no schema drift', health.schema_drift !== true, health.schema_drift_hint);
  const panesBefore = new Set((JSON.stringify(await tool('list_panes', {})).match(/[0-9a-f]{8}-[0-9a-f-]{27}/g) ?? []));

  for (const backend of backends) {
    const extra = backend === 'cursor_agent' ? { trust_workspace: true } : {};
    for (const [name, scenario] of Object.entries(scenarios)) {
      if (only && !only.includes(name)) continue;
      for (let round = 1; round <= repeat; round += 1) {
        const before = failures.length;
        const started = Date.now();
        try {
          await scenario(backend, name === 'visible_close' ? undefined : extra);
        } catch (error) {
          failures.push(`${name}: threw ${error.message}`);
        }
        const failed = failures.length > before;
        results.push({ backend, scenario: name, round, ok: !failed, seconds: ((Date.now() - started) / 1000).toFixed(1) });
        console.log(`${failed ? 'FAIL' : 'ok  '} ${backend} ${name} #${round} (${((Date.now() - started) / 1000).toFixed(1)}s)`);
        if (failed) failures.slice(before).forEach((line) => console.log(`       - ${line}`));
      }
    }
  }

  // Global invariants: nothing the suite started is still running, and no pane was leaked.
  for (const handle of new Set(handlesSeen)) {
    const view = await tool('inspect_agent', { handle, project_path: sandbox });
    check(`handle ${handle} settled`, view.status && view.status !== 'running', view.status);
  }
  const panesAfter = JSON.stringify(await tool('list_panes', {})).match(/[0-9a-f]{8}-[0-9a-f-]{27}/g) ?? [];
  const leaked = panesAfter.filter((id) => !panesBefore.has(id));
  check('no leaked panes', leaked.length === 0, leaked.join(', '));
}

try {
  await main();
} finally {
  proc.kill();
  // A backend process can still hold the folder open for a moment; leaving it behind is harmless.
  if (ownSandbox && !args.keep) {
    try { rmSync(sandbox, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 }); } catch { /* best effort */ }
  }
}
const passed = results.filter((entry) => entry.ok).length;
console.log(`\n${passed}/${results.length} scenario runs passed; ${failures.length} failed assertion(s)`);
if (failures.length) {
  failures.forEach((line) => console.log(` - ${line}`));
  process.exit(1);
}
