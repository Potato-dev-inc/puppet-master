// Live repro for docs/features/fix-live-claude.md. Needs a RUNNING Puppet Master build that contains
// the fix and a trusted demo project. It spawns real Claude Code panes (uses the subscription), so
// it is manual: `node scripts/evidence/live-claude-dialogs.mjs <demo-project> [bridge-url]`.
//
// 1. Trust gate: a visible run in a brand new folder must park on a workspace_trust card, type
//    nothing, and carry on after `trust`. A second run answered `deny` must fail with
//    WORKSPACE_TRUST_DECLINED and must NOT fall back to headless.
// 2. Permission card: a run whose task makes Claude ask for a Bash command must expose a
//    command_approval card (command, risk, allow_once / deny) and `answer_prompt` must work.
//
// The script answers prompts only in folders and for commands it created itself.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const project = process.argv[2];
const bridge = process.argv[3] ?? 'http://127.0.0.1:17321';
if (!project) throw new Error('usage: live-claude-dialogs.mjs <trusted demo project> [bridge url]');
const headers = { 'content-type': 'application/json', 'x-puppet-master-session': 'live-claude-dialogs' };
const call = async (method, route, body) => {
  const response = await fetch(bridge + route, { method, headers, body: body ? JSON.stringify(body) : undefined });
  return { status: response.status, body: await response.json().catch(() => null) };
};
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const run = (cwd, task, extra = {}) =>
  call('POST', '/agents/run', { project_path: cwd, agent_type: 'claude', task, headless: false, background: true, wait_ms: 1000, keep_pane: false, ...extra });
const view = async (cwd, id) => (await call('GET', `/operations/${id}?project_path=${encodeURIComponent(cwd)}`)).body;
async function until(cwd, id, test, what, ms = 90_000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const op = await view(cwd, id);
    if (test(op)) return op;
    await sleep(700);
  }
  throw new Error(`timed out waiting for ${what}`);
}
const fresh = (name) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `pm-live-${name}-`));
  return dir.replace(/\\/g, '/');
};
const report = (ok, text) => console.log(`${ok ? 'PASS' : 'FAIL'}  ${text}`);

// ---- 1a. trust: Trust --------------------------------------------------------------------------
const trusted = fresh('trust');
let started = await run(trusted, 'Reply with only the word PONG. Use no tools.');
let op = await until(trusted, started.body.operation_id, (o) => o.required_action?.kind === 'workspace_trust', 'the trust card');
report(op.status === 'waiting_input', `run parked on the trust gate (status ${op.status})`);
report(op.required_action.options.map((o) => o.id).join() === 'trust,deny', `options ${JSON.stringify(op.required_action.options)}`);
report(!op.worker?.fallback, 'no headless fallback while parked');
await call('POST', '/agents/answer', { handle: op.agent_run_id, project_path: trusted, prompt_id: op.required_action.prompt_id, choice: 'trust' });
op = await until(trusted, started.body.operation_id, (o) => ['completed', 'failed'].includes(o.status), 'the run to finish', 180_000);
report(op.status === 'completed' && /PONG/.test(op.result ?? ''), `after Trust the task ran: ${op.status} ${JSON.stringify(op.result)}`);
report(!op.worker?.fallback, 'ran in the visible pane (no fallback)');

// ---- 1b. trust: Deny ---------------------------------------------------------------------------
const refused = fresh('deny');
started = await run(refused, 'Reply with only the word PONG. Use no tools.');
op = await until(refused, started.body.operation_id, (o) => o.required_action?.kind === 'workspace_trust', 'the trust card (deny run)');
await call('POST', '/agents/answer', { handle: op.agent_run_id, project_path: refused, prompt_id: op.required_action.prompt_id, choice: 'deny' });
op = await until(refused, started.body.operation_id, (o) => ['completed', 'failed'].includes(o.status), 'the declined run to fail');
report(op.status === 'failed' && op.error?.code === 'WORKSPACE_TRUST_DECLINED', `declined: ${op.status} ${op.error?.code}`);
report(!op.worker?.fallback, 'declined run did not fall back to headless');

// ---- 2. Bash permission card -------------------------------------------------------------------
started = await run(project, 'Run exactly this Bash command and nothing else: cd . 2>/dev/null || cd "."; pwd; ls -la');
op = await until(project, started.body.operation_id, (o) => o.required_action?.kind === 'command_approval', 'the command card');
const card = op.required_action;
report(Boolean(card.command), `command extracted: ${card.command}`);
report(Boolean(card.risk?.headline), `risk: ${JSON.stringify(card.risk)}`);
report(card.options.map((o) => o.id).includes('allow_once') && card.options.map((o) => o.id).includes('deny'), 'allow_once and deny offered');
await call('POST', '/agents/answer', { handle: op.agent_run_id, project_path: project, prompt_id: card.prompt_id, choice: 'deny' });
op = await until(project, started.body.operation_id, (o) => o.required_action == null, 'the dialog to clear');
report(true, 'answered deny: the dialog cleared');
await call('POST', '/agents/cancel', { handle: op.agent_run_id, project_path: project });
