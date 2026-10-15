// Before/after medians per backend from two perf-latency.mjs runs.
//   node scripts/perf-compare.mjs <before-turns.json> <before-start-dir> <after-turns.json> <after-start-dir>
// Prints markdown: total latency, the stage breakdown, and the startup share a warm pool could remove.
import { existsSync, readFileSync } from 'node:fs';
import { frameAnalysis } from './perf-frames.mjs';

const [beforeFile, beforeStart, afterFile, afterStart] = process.argv.slice(2);

const median = (xs) => {
  const v = xs.filter((x) => typeof x === 'number' && Number.isFinite(x)).sort((a, b) => a - b);
  return v.length ? Math.round((v[Math.floor((v.length - 1) / 2)] + v[Math.ceil((v.length - 1) / 2)]) / 2) : null;
};
const diff = (a, b) => (typeof a === 'number' && typeof b === 'number' ? b - a : null);
const at = (row, stage) => row.stages.find(([name]) => name === stage)?.[1] ?? null;
const lastAt = (row, stage) => [...row.stages].reverse().find(([name]) => name === stage)?.[1] ?? null;

function load(file, startDir) {
  const data = JSON.parse(readFileSync(file, 'utf8'));
  const framesFile = file.replace(/-turns\.json$/, '-frames.json');
  if (existsSync(framesFile)) {
    const frames = JSON.parse(readFileSync(framesFile, 'utf8'));
    let counter = Number(startDir);
    for (const row of data.turns) {
      if (row.kind === 'cold') counter += 1;
      const key = `${row.scenario}/${row.dir_n ?? counter}/${row.kind === 'cold' ? 'cold' : `followup${row.index}`}`;
      if (!frames[key] || row.first_frame_ms === undefined) continue;
      const fa = frameAnalysis(frames[key], row.call_start, row.finished_at_ms);
      Object.assign(row, fa, {
        reply_visible_to_finished_ms: fa.reply_visible_at && row.finished_at_ms ? row.finished_at_ms - fa.reply_visible_at : null,
        screen_final_to_finished_ms: fa.screen_final_at && row.finished_at_ms ? row.finished_at_ms - fa.screen_final_at : null,
      });
    }
  }
  return data.turns;
}

function parts(row) {
  const headless = row.first_frame_ms === undefined;
  const own = { ...row, stages: row.stages.filter(([, t]) => t >= (row.create_off_ms ?? 0) - 1) };
  const out = { total: row.wall_total_ms, mcp_and_create: row.create_off_ms, return: row.return_lag_ms };
  const waitingReady = at(own, 'waiting_for_pane_readiness');
  const waitingInput = at(own, 'waiting_input');
  const dispatching = lastAt(own, 'dispatching');
  const dispatched = lastAt(own, 'dispatched_waiting_for_task_completion');
  if (headless) {
    const running = at(own, 'process_running');
    const first = at(own, 'first_output');
    out.spawn = diff(at(own, 'dispatching'), running);
    out.cli_to_first_output = diff(running, first);
    out.first_output_to_done = diff(first, row.finished_off_ms);
  } else {
    out.pane_spawn = diff(at(own, 'spawning_pane'), waitingReady);
    const trust = waitingInput !== null && dispatching !== null && waitingInput < dispatching;
    out.tui_ready = diff(waitingReady, trust ? waitingInput : dispatching);
    out.trust_to_dispatch = trust ? diff(waitingInput, dispatching) : 0;
    out.delivery = diff(dispatching, dispatched);
    out.model = diff(dispatched, row.reply_visible_ms);
    out.completion_lag = row.screen_final_to_finished_ms ?? row.finished_off_ms - (row.reply_visible_ms ?? row.finished_off_ms);
  }
  return out;
}

function summarize(turns) {
  const groups = new Map();
  for (const row of turns) {
    if (row.status !== 'completed') continue;
    const key = `${row.scenario}|${row.kind}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(parts(row));
  }
  const table = new Map();
  for (const [key, rows] of groups) {
    const keys = [...new Set(rows.flatMap((r) => Object.keys(r)))];
    table.set(key, { n: rows.length, ...Object.fromEntries(keys.map((k) => [k, median(rows.map((r) => r[k]))])) });
  }
  return table;
}

const before = summarize(load(beforeFile, beforeStart));
const after = summarize(load(afterFile, afterStart));
const failedBefore = JSON.parse(readFileSync(beforeFile, 'utf8')).turns.filter((r) => r.status !== 'completed');
const scenarios = [...new Set([...before.keys(), ...after.keys()].map((k) => k.split('|')[0]))];
const s = (ms) => (ms === null || ms === undefined ? 'n/a' : `${(ms / 1000).toFixed(1)}s`);

console.log('### Median wall-clock per turn (MCP call to returned result)\n');
console.log('| scenario | turn | before | after | saved | n (b/a) |\n|---|---|---|---|---|---|');
for (const name of scenarios) {
  for (const kind of ['cold', 'followup']) {
    const b = before.get(`${name}|${kind}`);
    const a = after.get(`${name}|${kind}`);
    if (!b && !a) continue;
    const saved = b && a ? s(b.total - a.total) : 'n/a';
    console.log(`| ${name} | ${kind} | ${s(b?.total)} | ${s(a?.total)} | ${saved} | ${b?.n ?? 0}/${a?.n ?? 0} |`);
  }
}
const cols = ['mcp_and_create', 'spawn', 'pane_spawn', 'tui_ready', 'trust_to_dispatch', 'delivery', 'model', 'cli_to_first_output', 'first_output_to_done', 'completion_lag', 'return'];
console.log('\n### Stage breakdown (median ms; before -> after)\n');
console.log(`| scenario | turn | ${cols.join(' | ')} |\n|---|---|${cols.map(() => '---').join('|')}|`);
for (const name of scenarios) {
  for (const kind of ['cold', 'followup']) {
    const b = before.get(`${name}|${kind}`);
    const a = after.get(`${name}|${kind}`);
    if (!b && !a) continue;
    const cell = (c) => (b?.[c] === undefined && a?.[c] === undefined ? '' : `${b?.[c] ?? '-'} -> ${a?.[c] ?? '-'}`);
    console.log(`| ${name} | ${kind} | ${cols.map(cell).join(' | ')} |`);
  }
}
console.log('\n### Warm-pool estimate (startup share of a cold turn, after the optimizations)\n');
console.log('| scenario | cold median | follow-up median | startup share (cold - follow-up) | stage-based (spawn + TUI ready + dialog + settle) |\n|---|---|---|---|---|');
for (const name of scenarios) {
  const cold = after.get(`${name}|cold`);
  const follow = after.get(`${name}|followup`);
  if (!cold) continue;
  const stage = ['spawn', 'pane_spawn', 'tui_ready', 'trust_to_dispatch'].reduce((sum, c) => sum + (cold[c] ?? 0), 0);
  console.log(`| ${name} | ${s(cold.total)} | ${s(follow?.total)} | ${follow ? s(cold.total - follow.total) : 'n/a'} | ${s(stage)} |`);
}
if (failedBefore.length) {
  console.log('\nNot completed in the before run: ' + [...new Set(failedBefore.map((r) => `${r.scenario} (${r.error ?? r.status})`))].join(', '));
}
