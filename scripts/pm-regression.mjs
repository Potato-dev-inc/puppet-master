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
//
// BENCHMARK MODE (--bench): compares backends on small native-subagent-style tasks and multi-file
// tasks against a generated fixture project, with timing and machine-verified correctness.
//
//   node scripts/pm-regression.mjs --bench [--backends opencode_native,cursor_agent,codex,claude]
//        [--tasks small,multi | pong,fix_bug,...] [--repeat 5] [--bench-out results.json]
//        [--max-p95 120] [--task-timeout 600] [--project dir] [--dry-run]
//
//   --bench          run the benchmark instead of the regression scenarios
//   --backends       default in bench mode: the backends (opencode_native, cursor_agent, codex,
//                    claude) whose CLI binary is found on PATH
//   --tasks          groups (small, multi) and/or task ids; default small,multi
//   --repeat N       runs per (backend, task); small tasks also run one warm followup per cold run
//   --bench-out f    write the JSON report (default pm-bench-<timestamp>.json in the cwd)
//   --max-p95 S      optional budget: fail when any group's p95 total wall time exceeds S seconds
//   --task-timeout S per-run wall limit before the run is cancelled (default 180 small / 600 multi)
//   --dry-run        print the plan and self-check the fixtures and checks with reference solutions;
//                    never spawns the MCP binary or any backend
//
// Bench exit code 1 when a run did not complete, a machine check fails locally, PM reported
// verified:true while the local check disagrees, or --max-p95 is exceeded. Results are per-machine:
// see docs/orchestrator/benchmark.md.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { delimiter, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, index, all) => {
    if (arg.startsWith('--')) pairs.push([arg.slice(2), all[index + 1]?.startsWith('--') || all[index + 1] === undefined ? true : all[index + 1]]);
    return pairs;
  }, []),
);
const bench = Boolean(args.bench || args['dry-run']); // --dry-run implies the bench plan
const dryRun = Boolean(args['dry-run']);
// Which CLI proves a backend is installed; used only to choose the default backend list in bench mode.
const BACKEND_BINARIES = { opencode_native: ['opencode'], cursor_agent: ['cursor-agent', 'agent'], codex: ['codex'], claude: ['claude'] };
function onPath(names) {
  const exts = process.platform === 'win32' ? (process.env.PATHEXT ?? '.EXE;.CMD;.BAT').split(';').concat('') : [''];
  for (const dir of (process.env.PATH ?? '').split(delimiter)) {
    for (const name of names) for (const ext of exts) if (dir && existsSync(join(dir, name + ext))) return join(dir, name + ext);
  }
  return null;
}
const defaultBackends = bench ? Object.keys(BACKEND_BINARIES).filter((name) => onPath(BACKEND_BINARIES[name])) : ['opencode_native', 'cursor_agent'];
const backends = (args.backends ? String(args.backends).split(',') : defaultBackends).filter(Boolean);
const repeat = Number(args.repeat ?? 1);
const only = args.only ? String(args.only).split(',') : null;
const binary = String(args.binary ?? join(root, 'packages', 'mcp-server', 'dist', process.platform === 'win32' ? 'puppet-master-mcp.exe' : 'puppet-master-mcp'));
if (!dryRun && !existsSync(binary)) {
  console.error(`MCP binary not found: ${binary}`);
  process.exit(2);
}

// --project reuses an existing folder (e.g. one Cursor already trusts, for visible-pane runs); it is never deleted.
const ownSandbox = !args.project;
const sandbox = ownSandbox ? mkdtempSync(join(tmpdir(), 'pm-regression-')) : String(args.project);
const proc = dryRun ? null : spawn(binary, [], { stdio: ['pipe', 'pipe', 'inherit'], windowsHide: true });
const pending = new Map();
let buffer = '';
let nextId = 1;
proc?.stdout.on('data', (chunk) => {
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
};

// ---------------------------------------------------------------------------------------------
// Benchmark mode
// ---------------------------------------------------------------------------------------------
const node = process.execPath;
const sha1 = (text) => createHash('sha1').update(text).digest('hex');

// Generated fixture: a tiny CommonJS project. Every run gets a fresh copy so runs never see each
// other's edits. Tasks may seed extra state (a bug, duplicated code) via `setup`.
const FIXTURE = {
  'package.json': '{ "name": "pm-bench-fixture", "version": "1.0.0", "private": true }\n',
  'src/math.js': `function add(a, b) { return a + b; }
function multiply(a, b) { return a * b; }
function clamp(n, lo, hi) { return Math.min(hi, Math.max(lo, n)); }
function isEven(n) { return n % 2 === 0; }
module.exports = { add, multiply, clamp, isEven };
`,
  'src/format.js': `function formatPrice(cents) { return \`$\${(cents / 100).toFixed(2)}\`; }
function titleCase(text) {
  return text.split(' ').map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join(' ');
}
module.exports = { formatPrice, titleCase };
`,
  'src/cart.js': `const { add, multiply } = require('./math');

function createCart() { return { items: [] }; }
function addItem(cart, item) {
  cart.items.push({ name: item.name, price: item.price, qty: item.qty ?? 1 });
  return cart;
}
function total(cart) {
  return cart.items.reduce((sum, item) => add(sum, multiply(item.price, item.qty)), 0);
}
module.exports = { createCart, addItem, total };
`,
  'src/report.js': `const { total } = require('./cart');
const { formatPrice, titleCase } = require('./format');
const { multiply } = require('./math');

function summarize(cart) {
  const lines = cart.items.map((item) => \`\${titleCase(item.name)} x\${item.qty} \${formatPrice(multiply(item.price, item.qty))}\`);
  lines.push(\`Total: \${formatPrice(total(cart))}\`);
  return lines.join('\\n');
}
module.exports = { summarize };
`,
  'test/math.test.js': `const test = require('node:test');
const assert = require('node:assert');
const { add, multiply, clamp } = require('../src/math');

test('add', () => assert.strictEqual(add(2, 3), 5));
test('multiply', () => assert.strictEqual(multiply(4, 5), 20));
test('clamp', () => assert.strictEqual(clamp(15, 0, 10), 10));
`,
  'test/cart.test.js': `const test = require('node:test');
const assert = require('node:assert');
const { createCart, addItem, total } = require('../src/cart');

test('total multiplies price by quantity', () => {
  const cart = addItem(createCart(), { name: 'widget', price: 250, qty: 3 });
  assert.strictEqual(total(cart), 750);
});
test('total sums items and defaults qty to 1', () => {
  const cart = addItem(addItem(createCart(), { name: 'a', price: 100 }), { name: 'b', price: 50, qty: 2 });
  assert.strictEqual(total(cart), 200);
});
`,
  'test/report.test.js': `const test = require('node:test');
const assert = require('node:assert');
const { createCart, addItem } = require('../src/cart');
const { summarize } = require('../src/report');

test('summarize lists items and a total', () => {
  const cart = addItem(createCart(), { name: 'blue widget', price: 1000, qty: 1 });
  assert.strictEqual(summarize(cart), 'Blue Widget x1 $10.00\\nTotal: $10.00');
});
`,
};
const CART_TEST_SHA = sha1(FIXTURE['test/cart.test.js']);

function writeFiles(dir, files) {
  for (const [rel, text] of Object.entries(files)) {
    const full = join(dir, rel);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, text);
  }
}
function edit(dir, rel, from, to) {
  const full = join(dir, rel);
  const before = readFileSync(full, 'utf8');
  if (!before.includes(from)) throw new Error(`fixture edit missed in ${rel}: ${from}`);
  writeFileSync(full, before.replace(from, () => to));
}
const testsPass = { type: 'command_exits', command: [node, '--test'] };
const nodeEval = (script) => ({ type: 'command_exits', command: [node, '-e', script] });
const PONG_PROMPT = 'Reply with only the word PONG. Use no tools.';

// A task: id, size (small|multi), prompt, checks (machine checks, run by PM and again locally),
// optional setup(dir) that seeds the fixture, solve(dir) = a reference solution used only by
// --dry-run to prove the checks are satisfiable (it returns the reply text for result checks),
// and an optional warm followup for small tasks.
const TASKS = [
  {
    id: 'pong', size: 'small',
    prompt: PONG_PROMPT,
    checks: [{ type: 'result_contains', text: 'PONG' }],
    solve: () => 'PONG',
    warm: { task: 'Reply with only the word PING. Use no tools.', checks: [{ type: 'result_contains', text: 'PING' }], solve: () => 'PING' },
  },
  {
    id: 'create_file', size: 'small',
    prompt: 'Create a file named hello.txt in the current folder containing exactly the text REGRESSION-OK. Do nothing else.',
    checks: [{ type: 'file_contains', path: 'hello.txt', text: 'REGRESSION-OK' }],
    solve: (dir) => writeFiles(dir, { 'hello.txt': 'REGRESSION-OK' }),
    warm: {
      task: 'Create a file named warm.txt in the current folder containing exactly the text WARM-OK. Do nothing else.',
      checks: [{ type: 'file_contains', path: 'warm.txt', text: 'WARM-OK' }],
      solve: (dir) => writeFiles(dir, { 'warm.txt': 'WARM-OK' }),
    },
  },
  {
    id: 'rename_symbol', size: 'small',
    prompt: 'In src/math.js only, rename the function isEven to isEvenNumber (definition and export). Do not touch any other file.',
    checks: [
      { type: 'file_contains', path: 'src/math.js', text: 'isEvenNumber' },
      nodeEval("const m=require('./src/math');process.exit(typeof m.isEvenNumber==='function'&&!('isEven' in m)&&m.isEvenNumber(4)?0:1)"),
      testsPass,
    ],
    solve: (dir) => { const f = join(dir, 'src/math.js'); writeFileSync(f, readFileSync(f, 'utf8').replace(/\bisEven\b/g, 'isEvenNumber')); },
    warm: { task: 'Which file defines the function multiply? Reply with only its relative path.', checks: [{ type: 'result_contains', text: 'math.js' }], solve: () => 'src/math.js' },
  },
  {
    id: 'read_question', size: 'small',
    prompt: 'Read src/format.js and tell me what string formatPrice(1999) returns. Reply with only that string. Do not modify any file.',
    checks: [{ type: 'result_contains', text: '$19.99' }],
    solve: () => '$19.99',
    warm: { task: 'Now read src/math.js: what does clamp(15, 0, 10) return? Reply with only the number.', checks: [{ type: 'result_contains', text: '10' }], solve: () => '10' },
  },
  {
    id: 'rename_fn', size: 'multi',
    prompt: 'Rename the function multiply to mul everywhere in this project: its definition and export in src/math.js, and every use in src/cart.js, src/report.js and test/math.test.js. Behaviour must not change; run `node --test` to confirm it still passes.',
    checks: [
      { type: 'file_contains', path: 'src/math.js', text: 'mul' },
      nodeEval("const fs=require('fs');const bad=['src/math.js','src/cart.js','src/report.js','test/math.test.js'].filter((f)=>/\\bmultiply\\b/.test(fs.readFileSync(f,'utf8')));process.exit(bad.length?1:0)"),
      testsPass,
    ],
    solve: (dir) => { for (const f of ['src/math.js', 'src/cart.js', 'src/report.js', 'test/math.test.js']) { const p = join(dir, f); writeFileSync(p, readFileSync(p, 'utf8').replace(/\bmultiply\b/g, 'mul')); } },
  },
  {
    id: 'add_field', size: 'multi',
    prompt: 'Cart items should accept an optional string field `sku` (default null) that addItem stores on the item. Implement it in src/cart.js and add a test to test/cart.test.js that adds an item with a sku and asserts it is stored. Run `node --test` to confirm everything passes.',
    checks: [
      { type: 'file_contains', path: 'src/cart.js', text: 'sku' },
      { type: 'file_contains', path: 'test/cart.test.js', text: 'sku' },
      nodeEval("const {createCart,addItem}=require('./src/cart');const c=addItem(createCart(),{name:'a',price:1,sku:'X1'});const d=addItem(createCart(),{name:'b',price:1});process.exit(c.items[0].sku==='X1'&&d.items[0].sku===null?0:1)"),
      testsPass,
    ],
    solve: (dir) => {
      edit(dir, 'src/cart.js', 'qty: item.qty ?? 1 }', 'qty: item.qty ?? 1, sku: item.sku ?? null }');
      const f = join(dir, 'test/cart.test.js');
      writeFileSync(f, `${readFileSync(f, 'utf8')}\ntest('sku is stored', () => {\n  const cart = addItem(createCart(), { name: 'a', price: 1, sku: 'X1' });\n  assert.strictEqual(cart.items[0].sku, 'X1');\n});\n`);
    },
  },
  {
    id: 'fix_bug', size: 'multi',
    prompt: '`node --test` is failing. Find the bug in the source code under src/ and fix it. Do not edit anything under test/. Run `node --test` to confirm it passes.',
    setup: (dir) => edit(dir, 'src/cart.js', 'add(sum, multiply(item.price, item.qty))', 'add(sum, item.price)'),
    checks: [
      testsPass,
      nodeEval(`const c=require('crypto');const fs=require('fs');process.exit(c.createHash('sha1').update(fs.readFileSync('test/cart.test.js','utf8')).digest('hex')==='${CART_TEST_SHA}'?0:1)`),
    ],
    solve: (dir) => edit(dir, 'src/cart.js', 'add(sum, item.price)', 'add(sum, multiply(item.price, item.qty))'),
  },
  {
    id: 'add_module', size: 'multi',
    prompt: 'Add src/discount.js exporting applyDiscount(cents, percent): it returns cents reduced by percent (percent clamped to 0..100 using clamp from src/math.js), rounded to a whole number of cents. Wire it into src/report.js: summarize(cart, options = {}) must apply options.discountPercent (default 0) to the Total line only. Add test/discount.test.js covering applyDiscount. Run `node --test` to confirm everything passes.',
    checks: [
      { type: 'file_contains', path: 'src/discount.js', text: 'applyDiscount' },
      { type: 'file_contains', path: 'src/report.js', text: 'applyDiscount' },
      { type: 'file_contains', path: 'test/discount.test.js', text: 'applyDiscount' },
      nodeEval("const {applyDiscount}=require('./src/discount');const {createCart,addItem}=require('./src/cart');const {summarize}=require('./src/report');const c=addItem(createCart(),{name:'widget',price:1000,qty:1});process.exit(applyDiscount(1000,10)===900&&applyDiscount(1000,150)===0&&summarize(c,{discountPercent:10}).includes('Total: $9.00')&&summarize(c).includes('Total: $10.00')?0:1)"),
      testsPass,
    ],
    solve: (dir) => {
      writeFiles(dir, {
        'src/discount.js': "const { clamp } = require('./math');\nfunction applyDiscount(cents, percent) { return Math.round(cents * (1 - clamp(percent, 0, 100) / 100)); }\nmodule.exports = { applyDiscount };\n",
        'test/discount.test.js': "const test = require('node:test');\nconst assert = require('node:assert');\nconst { applyDiscount } = require('../src/discount');\n\ntest('applyDiscount', () => assert.strictEqual(applyDiscount(1000, 10), 900));\n",
      });
      edit(dir, 'src/report.js', "const { total } = require('./cart');", "const { total } = require('./cart');\nconst { applyDiscount } = require('./discount');");
      edit(dir, 'src/report.js', 'function summarize(cart) {', 'function summarize(cart, options = {}) {');
      edit(dir, 'src/report.js', 'formatPrice(total(cart))', 'formatPrice(applyDiscount(total(cart), options.discountPercent ?? 0))');
    },
  },
  {
    id: 'dedupe', size: 'multi',
    prompt: 'src/receipt.js and src/invoice.js each contain an identical private copy of the function formatLine. Move it into a new shared module src/line.js (exporting formatLine) and import it from both files. Keep the exported receiptLine and invoiceLine behaviour unchanged and do not edit tests. Run `node --test` to confirm it passes.',
    setup: (dir) => {
      const line = "function formatLine(item) { return `${titleCase(item.name)} x${item.qty} ${formatPrice(multiply(item.price, item.qty))}`; }";
      const head = "const { formatPrice, titleCase } = require('./format');\nconst { multiply } = require('./math');\n\n";
      writeFiles(dir, {
        'src/receipt.js': `${head}${line}\nfunction receiptLine(item) { return \`RECEIPT \${formatLine(item)}\`; }\nmodule.exports = { receiptLine };\n`,
        'src/invoice.js': `${head}${line}\nfunction invoiceLine(item) { return \`INVOICE \${formatLine(item)}\`; }\nmodule.exports = { invoiceLine };\n`,
        'test/lines.test.js': "const test = require('node:test');\nconst assert = require('node:assert');\nconst { receiptLine } = require('../src/receipt');\nconst { invoiceLine } = require('../src/invoice');\n\nconst item = { name: 'blue widget', price: 500, qty: 2 };\ntest('receiptLine', () => assert.strictEqual(receiptLine(item), 'RECEIPT Blue Widget x2 $10.00'));\ntest('invoiceLine', () => assert.strictEqual(invoiceLine(item), 'INVOICE Blue Widget x2 $10.00'));\n",
      });
    },
    checks: [
      { type: 'file_contains', path: 'src/line.js', text: 'formatLine' },
      nodeEval("const fs=require('fs');const n=fs.readdirSync('src').filter((f)=>f.endsWith('.js')).map((f)=>(fs.readFileSync('src/'+f,'utf8').match(/function formatLine/g)||[]).length).reduce((a,b)=>a+b,0);process.exit(n===1?0:1)"),
      testsPass,
    ],
    solve: (dir) => {
      const lineSrc = "const { formatPrice, titleCase } = require('./format');\nconst { multiply } = require('./math');\n\nfunction formatLine(item) { return `${titleCase(item.name)} x${item.qty} ${formatPrice(multiply(item.price, item.qty))}`; }\nmodule.exports = { formatLine };\n";
      writeFiles(dir, { 'src/line.js': lineSrc });
      for (const [file, fn, tag] of [['src/receipt.js', 'receiptLine', 'RECEIPT'], ['src/invoice.js', 'invoiceLine', 'INVOICE']]) {
        writeFiles(dir, { [file]: `const { formatLine } = require('./line');\n\nfunction ${fn}(item) { return \`${tag} \${formatLine(item)}\`; }\nmodule.exports = { ${fn} };\n` });
      }
    },
  },
];

const TASK_GROUPS = { small: TASKS.filter((t) => t.size === 'small').map((t) => t.id), multi: TASKS.filter((t) => t.size === 'multi').map((t) => t.id) };
function selectedTasks() {
  const wanted = String(args.tasks ?? 'small,multi').split(',').filter(Boolean).flatMap((name) => TASK_GROUPS[name] ?? [name]);
  const unknown = wanted.filter((id) => !TASKS.some((t) => t.id === id));
  if (unknown.length) { console.error(`unknown task(s): ${unknown.join(', ')} (known: ${TASKS.map((t) => t.id).join(', ')})`); process.exit(2); }
  return TASKS.filter((t) => wanted.includes(t.id));
}

function freshFixture(dir, task) {
  rmSync(dir, { recursive: true, force: true });
  writeFiles(dir, FIXTURE);
  task.setup?.(dir);
}

/** Ground-truth evaluation of one machine check against the fixture dir (and the run's reply text). */
function localCheck(dir, check, result = '') {
  try {
    if (check.type === 'result_contains') return String(result ?? '').includes(check.text);
    if (check.type === 'file_contains') return readFileSync(join(dir, check.path), 'utf8').includes(check.text);
    if (check.type === 'command_exits') {
      const run = spawnSync(check.command[0], check.command.slice(1), { cwd: dir, encoding: 'utf8', timeout: 120_000, windowsHide: true });
      return (run.status ?? -1) === (check.expect_exit ?? 0);
    }
  } catch { /* missing file etc. */ }
  return false;
}
const allLocal = (dir, checks, result) => checks.every((check) => localCheck(dir, check, result));

function percentile(sorted, p) {
  if (!sorted.length) return null;
  const rank = (sorted.length - 1) * p;
  const lo = Math.floor(rank);
  const hi = Math.ceil(rank);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo);
}
const fmt = (value) => (value === null || value === undefined || Number.isNaN(value) ? '-' : Number(value).toFixed(1));

function summarizeRuns(runs) {
  const groups = new Map();
  for (const run of runs) {
    const key = `${run.backend}|${run.task}|${run.phase}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(run);
  }
  return [...groups.entries()].map(([key, rows]) => {
    const [backend, task, phase] = key.split('|');
    const totals = rows.map((r) => r.total_s).sort((a, b) => a - b);
    const handles = rows.map((r) => r.ttfh_s).filter((v) => v !== null).sort((a, b) => a - b);
    return {
      backend, task, phase, n: rows.length,
      p50_s: percentile(totals, 0.5), p95_s: percentile(totals, 0.95), min_s: totals[0], max_s: totals[totals.length - 1],
      ttfh_p50_s: percentile(handles, 0.5),
      pass_rate: rows.filter((r) => r.pass).length / rows.length,
      verified_rate: rows.filter((r) => r.verified).length / rows.length,
    };
  });
}
function printTable(summary) {
  const cols = ['backend', 'task', 'phase', 'n', 'p50', 'p95', 'min', 'max', 'ttfh50', 'pass', 'verified'];
  const rows = summary.map((s) => [s.backend, s.task, s.phase, String(s.n), fmt(s.p50_s), fmt(s.p95_s), fmt(s.min_s), fmt(s.max_s), fmt(s.ttfh_p50_s), `${Math.round(s.pass_rate * 100)}%`, `${Math.round(s.verified_rate * 100)}%`]);
  const widths = cols.map((c, i) => Math.max(c.length, ...rows.map((r) => r[i].length)));
  const line = (cells) => cells.map((c, i) => (i < 3 ? c.padEnd(widths[i]) : c.padStart(widths[i]))).join('  ');
  console.log(`\n${line(cols)}\n${widths.map((w) => '-'.repeat(w)).join('  ')}`);
  rows.forEach((r) => console.log(line(r)));
  console.log('(seconds; p50/p95/min/max = total wall time, ttfh50 = median time to first handle)');
}

const TERMINAL = ['completed', 'failed', 'cancelled'];
const seconds = (ms) => ms / 1000;
const stamp = (value) => { const t = Date.parse(value); return Number.isNaN(t) ? null : t; };

/** Wait until the handle's turn is terminal or the deadline passes (then cancel it). */
async function settleRun(view, handle, dir, limitMs) {
  const end = Date.now() + limitMs;
  while (!TERMINAL.includes(view.status) && handle && Date.now() < end) {
    const waited = await tool('wait_agents', { handles: [handle], timeout_ms: Math.min(60_000, Math.max(1000, end - Date.now())), project_path: dir });
    view = waited.runs?.[0] ?? waited.agents?.[0] ?? view;
    if (view._error) break;
  }
  if (!TERMINAL.includes(view.status)) {
    if (handle) await tool('cancel_agent', { handle, project_path: dir });
    view = { ...view, status: 'timeout' };
  }
  return view;
}

async function benchPhase({ backend, task, round, phase, dir, begin, handleOf, checks, limitMs }) {
  const t0 = Date.now();
  let view = await begin();
  const handle = view.handle ?? handleOf;
  const ttfh = handle ? seconds(Date.now() - t0) : null;
  if (handle) handlesSeen.push(handle);
  view = await settleRun(view, handle, dir, limitMs);
  const totalS = seconds(Date.now() - t0);
  const inspected = handle ? await tool('inspect_agent', { handle, project_path: dir }) : {};
  const pick = (key) => view[key] ?? inspected[key] ?? inspected.run?.[key] ?? inspected.turn?.[key] ?? null;
  const startedAt = pick('started_at');
  const completedAt = pick('completed_at');
  // Prefer the run view's own duration_ms (prompt written -> completion); started/completed stamps
  // are not exposed on every build.
  const stampedMs = stamp(startedAt) !== null && stamp(completedAt) !== null ? stamp(completedAt) - stamp(startedAt) : null;
  const serverMs = typeof view.duration_ms === 'number' ? view.duration_ms : stampedMs;
  const result = view.result ?? inspected.result ?? '';
  const completed = view.status === 'completed';
  const localOk = completed && allLocal(dir, checks, result);
  const pmVerified = view.verified === true || view.acceptance_status === 'passed';
  const row = {
    backend, task: task.id, size: task.size, phase, round, handle: handle ?? null, status: view.status,
    total_s: totalS, ttfh_s: ttfh, started_at: startedAt, completed_at: completedAt, server_s: serverMs === null ? null : seconds(serverMs),
    acceptance: view.acceptance_status ?? null, pm_verified: pmVerified, local_ok: localOk,
    pass: localOk, verified: localOk && pmVerified, error: view.error?.code ?? view._error ?? null,
  };
  const tag = `${backend}/${task.id}/${phase}#${round}`;
  if (!completed) failures.push(`${tag}: did not complete (${view.status} ${row.error ?? ''}${view.error?.context ? ` ${JSON.stringify(view.error.context)}` : ''})`);
  else if (!localOk) failures.push(`${tag}: local machine check failed (PM acceptance ${row.acceptance ?? 'n/a'})`);
  if (pmVerified && completed && !localOk) failures.push(`${tag}: PM reported verified but the local check disagrees`);
  console.log(`${row.pass ? 'ok  ' : 'FAIL'} ${tag} total ${fmt(totalS)}s ttfh ${fmt(ttfh)}s server ${fmt(row.server_s)}s verified=${row.verified}`);
  return { row, handle };
}

async function benchMain(health) {
  const picked = selectedTasks();
  if (!backends.length) { console.error('no backends: none of the known CLIs were found on PATH; pass --backends'); process.exit(2); }
  const runs = [];
  for (const backend of backends) {
    const extra = backend === 'cursor_agent' ? { trust_workspace: true } : {};
    for (const task of picked) {
      const limitMs = 1000 * Number(args['task-timeout'] ?? (task.size === 'small' ? 180 : 600));
      for (let round = 1; round <= repeat; round += 1) {
        const dir = join(sandbox, `bench-${backend}-${task.id}-${round}`);
        let handle = null;
        try {
          freshFixture(dir, task);
          const cold = await benchPhase({
            backend, task, round, phase: 'cold', dir, limitMs, checks: task.checks,
            begin: () => tool('run_agent', { agent_type: backend, project_path: dir, task: task.prompt, checks: task.checks, background: true, wait_ms: 0, keep_pane: Boolean(task.size === 'small' && task.warm), ...extra }),
          });
          runs.push(cold.row);
          handle = cold.handle;
          if (task.size === 'small' && task.warm && cold.row.status === 'completed' && handle) {
            const warm = task.warm;
            const second = await benchPhase({
              backend, task, round, phase: 'warm', dir, limitMs, checks: warm.checks, handleOf: handle,
              begin: async () => {
                let view = await tool('followup_task', { handle, task: warm.task, checks: warm.checks, wait_ms: 0, project_path: dir });
                if (view._error) view = await tool('followup_task', { handle, task: warm.task, wait_ms: 0, project_path: dir });
                return view;
              },
            });
            runs.push(second.row);
          }
        } catch (error) {
          failures.push(`${backend}/${task.id}#${round}: threw ${error.message}`);
        } finally {
          if (handle) await tool('close_agent', { handle, project_path: dir });
        }
      }
    }
  }
  const summary = summarizeRuns(runs);
  printTable(summary);
  const budget = args['max-p95'] === undefined ? null : Number(args['max-p95']);
  if (budget !== null && Number.isFinite(budget)) {
    for (const s of summary) if (s.p95_s > budget) failures.push(`p95 budget: ${s.backend}/${s.task}/${s.phase} p95 ${fmt(s.p95_s)}s > ${budget}s`);
  }
  const outFile = String(args['bench-out'] === true || args['bench-out'] === undefined ? `pm-bench-${new Date().toISOString().replace(/[:.]/g, '-')}.json` : args['bench-out']);
  writeFileSync(outFile, `${JSON.stringify({
    meta: { date: new Date().toISOString(), build_id: health?.build_id ?? null, node: process.version, platform: `${process.platform}-${process.arch}`, backends, tasks: picked.map((t) => t.id), repeat, max_p95_s: budget },
    runs, summary,
  }, null, 2)}\n`);
  console.log(`wrote ${outFile}`);
}

/** --dry-run: print the plan and prove every task's checks fail on the pristine fixture and pass after a reference solution. */
async function dryRunMain() {
  const picked = selectedTasks();
  console.log('PM benchmark dry run (no MCP binary or backend is started)');
  console.log(`backends: ${backends.length ? backends.map((b) => `${b}${BACKEND_BINARIES[b] && !onPath(BACKEND_BINARIES[b]) ? ' (binary not on PATH)' : ''}`).join(', ') : '(none found on PATH)'}`);
  console.log(`repeat: ${repeat}  max-p95: ${args['max-p95'] ?? 'none'}  out: ${args['bench-out'] ?? 'pm-bench-<timestamp>.json'}`);
  const small = picked.filter((t) => t.size === 'small').length;
  const perBackend = (picked.length + small) * repeat;
  console.log(`runs: ${perBackend} per backend (${picked.length} tasks x ${repeat}, plus ${small * repeat} warm followups) = ${perBackend * backends.length} total\n`);
  let bad = 0;
  for (const task of picked) {
    const pristine = join(sandbox, `dry-${task.id}-pristine`);
    const solved = join(sandbox, `dry-${task.id}-solved`);
    freshFixture(pristine, task);
    freshFixture(solved, task);
    let reply = '';
    try { reply = task.solve(solved) ?? ''; } catch (error) { console.log(`  solve threw: ${error.message}`); bad += 1; }
    const before = task.checks.map((check) => localCheck(pristine, check, ''));
    const after = task.checks.map((check) => localCheck(solved, check, reply));
    const resultOnly = task.checks.every((check) => check.type === 'result_contains');
    // fix_bug's hash guard is meant to hold on the pristine tree; every other check set must not pass untouched.
    const ok = after.every(Boolean) && (resultOnly || !before.every(Boolean));
    if (!ok) bad += 1;
    console.log(`${ok ? 'ok  ' : 'BAD '} ${task.id} [${task.size}] checks=${task.checks.length} pristine=${before.map((v) => (v ? 'pass' : 'fail')).join('/')} reference=${after.map((v) => (v ? 'pass' : 'fail')).join('/')}`);
    console.log(`       prompt: ${task.prompt}`);
    if (task.warm) {
      const warmDir = join(sandbox, `dry-${task.id}-warm`);
      freshFixture(warmDir, task);
      let warmReply = '';
      try { task.solve(warmDir); warmReply = task.warm.solve(warmDir) ?? ''; } catch (error) { console.log(`  warm solve threw: ${error.message}`); bad += 1; }
      const warmOk = allLocal(warmDir, task.warm.checks, warmReply);
      if (!warmOk) bad += 1;
      console.log(`       warm ${warmOk ? 'ok' : 'BAD'}: ${task.warm.task}`);
    }
  }
  console.log(`\nfixture files: ${Object.keys(FIXTURE).join(', ')}`);
  if (bad) failures.push(`${bad} dry-run self-check(s) failed`);
  else console.log('all fixture self-checks passed');
}

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

  if (bench) await benchMain(health);
  else for (const backend of backends) {
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
  if (dryRun) await dryRunMain();
  else await main();
} finally {
  proc?.kill();
  // A backend process can still hold the folder open for a moment; leaving it behind is harmless.
  if (ownSandbox && !args.keep) {
    try { rmSync(sandbox, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 }); } catch { /* best effort */ }
  }
}
if (bench) {
  if (failures.length) {
    console.log(`
${failures.length} benchmark failure(s)`);
    failures.forEach((line) => console.log(` - ${line}`));
    process.exit(1);
  }
  process.exit(0);
}
const passed = results.filter((entry) => entry.ok).length;
console.log(`\n${passed}/${results.length} scenario runs passed; ${failures.length} failed assertion(s)`);
if (failures.length) {
  failures.forEach((line) => console.log(` - ${line}`));
  process.exit(1);
}
