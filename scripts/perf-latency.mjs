// Delegation latency measurement against the running app (bridge on 127.0.0.1:17321 by default).
//
//   node scripts/perf-latency.mjs --out <dir> [--scenarios a,b] [--cold 3] [--followups 2] [--label before]
//
// For each scenario (backend x visible/headless) it runs a trivial PONG task in a fresh sandbox
// `<out>/perf-<n>`, then follow-ups on the same handle, and records per turn:
//   - wall-clock of every MCP call (the orchestrator's view)
//   - the operation snapshot's stage_log / created / started / finished timestamps (the app's view)
//   - for visible panes, a 40ms poll of the pane screen: first frame and the first frame where the
//     reply is readable, so completion-detection lag can be separated from model time.
// Raw output: <out>/<label>-turns.json, -frames.json and -turns.csv. Every handle is closed at the end.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createContext, PONG } from './pm-regression/context.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i >= 0 ? argv[i + 1] : fallback; };
const out = opt('out');
if (!out) { console.error('--out <dir> required'); process.exit(2); }
const label = opt('label', 'run');
const COLD = Number(opt('cold', 3));
const FOLLOWUPS = Number(opt('followups', 2));
const BRIDGE = opt('bridge', 'http://127.0.0.1:17321');
// --task / --allowed let an experiment run something longer than PONG (frames are still recorded).
const TASK = opt('task', PONG);
const ALLOWED = opt('allowed') ? opt('allowed').split(',') : null;
mkdirSync(out, { recursive: true });

const SCENARIOS = {
  cursor_visible: { agent_type: 'cursor_agent', headless: false, keep_pane: true },
  cursor_headless: { agent_type: 'cursor_agent', headless: true, trust_workspace: true },
  claude_visible: { agent_type: 'claude', headless: false, keep_pane: true },
  claude_headless: { agent_type: 'claude', headless: true },
  codex_headless: { agent_type: 'codex', headless: true },
  opencode_visible: { agent_type: 'opencode_native', headless: false, keep_pane: true },
  opencode_background: { agent_type: 'opencode_native', headless: true, background: true, keep_pane: true },
};
const wanted = opt('scenarios') ? opt('scenarios').split(',') : Object.keys(SCENARIOS);

const ctx = createContext(['--project', out]);
const { tool } = ctx;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let dirCounter = Number(opt('start-dir', 10));
const median = (xs) => {
  const v = xs.filter((x) => typeof x === 'number').sort((a, b) => a - b);
  return v.length ? (v[Math.floor((v.length - 1) / 2)] + v[Math.ceil((v.length - 1) / 2)]) / 2 : null;
};

async function http(path) {
  const t = Date.now();
  const res = await fetch(`${BRIDGE}${path}`);
  return { ms: Date.now() - t, body: await res.text(), status: res.status };
}

/** Observe the pane whose cwd is `dir` until stop(): records frames when the screen changes. */
function watchPane(dir) {
  const norm = (p) => p.replace(/\\/g, '/').toLowerCase();
  const frames = [];
  let stopped = false;
  let paneId = null;
  const loop = (async () => {
    let last = '';
    let lastStatus = '';
    while (!stopped) {
      try {
        const list = JSON.parse((await http('/panes')).body);
        const mine = list.find((p) => norm(p.cwd ?? '') === norm(dir));
        paneId = mine?.id ?? paneId;
        if (mine && mine.status !== lastStatus) { lastStatus = mine.status; frames.push({ t: Date.now(), status: mine.status, text: last }); }
        if (paneId) {
          const r = await http(`/panes/${paneId}/buffer?view=screen`);
          let text = r.body;
          try { const j = JSON.parse(r.body); text = j.content ?? j.text ?? j.screen ?? r.body; } catch { /* raw text */ }
          if (text !== last) { last = text; frames.push({ t: Date.now(), text }); }
        }
      } catch { /* pane gone */ }
      await sleep(40);
    }
  })();
  return { frames, get paneId() { return paneId; }, async stop() { stopped = true; await loop; return frames; } };
}

function readOperation(dir, id) {
  try { return JSON.parse(readFileSync(join(dir, '.puppet-master', 'operations', `${id}.json`), 'utf8')); } catch { return null; }
}

import { frameAnalysis } from './perf-frames.mjs';

async function settle(view, dir, notes) {
  for (let i = 0; i < 40; i += 1) {
    const terminal = ['completed', 'failed', 'cancelled', 'interrupted', 'closed'].includes(view.status);
    if (terminal || !view.handle) break;
    if (view.status === 'needs_input' && view.prompt) {
      const ta = Date.now();
      if (view.prompt.kind !== 'workspace_trust') { notes.push(`blocked:${view.prompt.kind}`); break; }
      await tool('answer_prompt', { handle: view.handle, prompt_id: view.prompt.id, choice: 'trust', project_path: dir });
      notes.push(`answered_trust_ms=${Date.now() - ta}`);
    }
    const waited = await tool('wait_agents', { handles: [view.handle], timeout_ms: 120_000, project_path: dir });
    view = waited.runs?.[0] ?? waited.agents?.[0] ?? view;
  }
  return view;
}

async function oneTurn({ scenario, cfg, dir, kind, index, handle }) {
  const notes = [];
  const visible = cfg.headless === false;
  const watcher = visible ? watchPane(dir) : null;
  const tCall = Date.now();
  let view;
  if (kind === 'cold') {
    view = await tool('run_agent', { ...cfg, project_path: dir, task: TASK, ...(ALLOWED ? { allowed_commands: ALLOWED } : {}), wait_ms: 120_000, idempotency_key: `perf-${label}-${dirCounter}-${Math.random().toString(36).slice(2)}` });
  } else {
    view = await tool('followup_task', { handle, project_path: dir, task: TASK, wait_ms: 120_000, background: false });
  }
  const tFirstReturn = Date.now();
  view = await settle(view, dir, notes);
  const tEnd = Date.now();
  const frames = watcher ? await watcher.stop() : [];
  const op = view.operation_id ? readOperation(dir, view.operation_id) : null;
  const created = op?.created_at_ms ?? null;
  const stages = op?.worker?.stage_log ?? [];
  const fa = visible ? frameAnalysis(frames, tCall, op?.finished_at_ms ?? null) : {};
  const row = {
    scenario, kind, index, dir_n: dirCounter, handle: view.handle ?? handle ?? null, operation_id: view.operation_id ?? null,
    status: view.status, error: view.error?.code ?? view._error ?? null, result: String(view.result ?? '').slice(0, 40),
    pane_id: view.pane_id ?? watcher?.paneId ?? null,
    call_start: tCall, wall_first_return_ms: tFirstReturn - tCall, wall_total_ms: tEnd - tCall,
    created_at_ms: created, started_at_ms: op?.started_at_ms ?? null, finished_at_ms: op?.finished_at_ms ?? null,
    create_off_ms: created ? created - tCall : null,
    started_off_ms: op?.started_at_ms ? op.started_at_ms - tCall : null,
    finished_off_ms: op?.finished_at_ms ? op.finished_at_ms - tCall : null,
    return_lag_ms: op?.finished_at_ms ? tEnd - op.finished_at_ms : null,
    screen_final_to_finished_ms: fa.screen_final_at && op?.finished_at_ms ? op.finished_at_ms - fa.screen_final_at : null,
    reply_visible_to_finished_ms: fa.reply_visible_at && op?.finished_at_ms ? op.finished_at_ms - fa.reply_visible_at : null,
    stages: stages.map((s) => [s.stage, s.at_ms - tCall]),
    setup_ms: view.setup_ms ?? null, duration_ms: view.duration_ms ?? null, total_ms: view.total_ms ?? null,
    ...fa, notes,
  };
  return { row, frames };
}

async function bridgeOverhead() {
  const direct = []; const shim = [];
  for (let i = 0; i < 25; i += 1) {
    direct.push((await http('/health')).ms);
    const t = Date.now(); await tool('bridge_health', {}); shim.push(Date.now() - t);
  }
  return { direct_health_median_ms: median(direct), shim_health_median_ms: median(shim) };
}

const all = []; const frameDump = {};
const overhead = await bridgeOverhead();
console.log('overhead', overhead);
for (const name of wanted) {
  const cfg = SCENARIOS[name];
  if (!cfg) { console.error(`unknown scenario ${name}`); continue; }
  for (let c = 0; c < COLD; c += 1) {
    dirCounter += 1;
    const dir = join(out, `perf-${dirCounter}`);
    mkdirSync(dir, { recursive: true });
    if (cfg.agent_type === 'codex') spawnSync('git', ['init', '-q'], { cwd: dir });
    const first = await oneTurn({ scenario: name, cfg, dir, kind: 'cold', index: c });
    all.push(first.row); frameDump[`${name}/${dirCounter}/cold`] = first.frames;
    console.log(name, 'cold', c, first.row.status, first.row.wall_total_ms, first.row.error ?? '');
    const handle = first.row.handle;
    if (first.row.status === 'completed') {
      for (let f = 0; f < FOLLOWUPS; f += 1) {
        const r = await oneTurn({ scenario: name, cfg, dir, kind: 'followup', index: f, handle });
        all.push(r.row); frameDump[`${name}/${dirCounter}/followup${f}`] = r.frames;
        console.log(name, 'followup', f, r.row.status, r.row.wall_total_ms, r.row.error ?? '');
      }
    }
    if (handle) { try { await tool('close_agent', { handle, project_path: dir }); } catch { /* best effort */ } }
  }
}

writeFileSync(join(out, `${label}-turns.json`), JSON.stringify({ label, at: new Date().toISOString(), overhead, turns: all }, null, 1));
writeFileSync(join(out, `${label}-frames.json`), JSON.stringify(frameDump));
const cols = ['scenario', 'kind', 'index', 'status', 'error', 'wall_first_return_ms', 'wall_total_ms', 'create_off_ms', 'started_off_ms', 'finished_off_ms', 'return_lag_ms', 'first_frame_ms', 'reply_visible_ms', 'reply_visible_to_finished_ms', 'screen_final_to_finished_ms', 'setup_ms', 'duration_ms', 'status_trace', 'stages'];
writeFileSync(join(out, `${label}-turns.csv`), [cols.join(','), ...all.map((r) => cols.map((c) => JSON.stringify(Array.isArray(r[c]) ? r[c].map((s) => s.join('@')).join(' ') : (r[c] ?? ''))).join(','))].join('\n'));
const sandboxPrefix = out.replace(/\\/g, '/').toLowerCase();
const left = JSON.parse((await http('/panes')).body).filter((p) => String(p.cwd).replace(/\\/g, '/').toLowerCase().startsWith(sandboxPrefix));
console.log('panes left in sandbox:', left.length);
ctx.proc.kill();
process.exit(0);
