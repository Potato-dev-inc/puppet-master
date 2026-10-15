// Turns the raw turns JSON written by perf-latency.mjs into the per-backend breakdown table.
//   node scripts/perf-summarize.mjs <label-turns.json> [--md] [--csv out.csv]
// Components (ms, offsets are from the orchestrator's MCP call start):
//   op_create     MCP call -> operation snapshot created (shim + HTTP + validation + persist)
//   pane_spawn    spawning_pane -> waiting_for_pane_readiness (process launch + registration)
//   tui_ready     waiting_for_pane_readiness -> first waiting_input or dispatching (until the pane is dispatch-ready)
//   trust_wait    time parked in waiting_input on a startup dialog (includes the orchestrator's answer round trip)
//   settle        end of the startup dialog -> dispatching (pane idle + screen holds still)
//   delivery      dispatching -> dispatched_waiting_for_task_completion (typing + submit verification)
//   model         dispatched -> reply readable on screen (visible) / first_output (headless)
//   completion    reply readable -> operation completed (visible) / first_output -> completed (headless)
//   return        operation completed -> orchestrator call returned
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { frameAnalysis } from './perf-frames.mjs';

const [file, ...rest] = process.argv.slice(2);
const data = JSON.parse(readFileSync(file, 'utf8'));
const md = rest.includes('--md');
// Recompute the screen-derived fields from the frame dump written next to the turns file, so the
// analysis can be refined without re-running the measurement. Keys are scenario/<dir n>/<cold|followupN>.
const framesFile = file.replace(/-turns\.json$/, '-frames.json');
if (existsSync(framesFile)) {
  const frames = JSON.parse(readFileSync(framesFile, 'utf8'));
  const startDir = Number(rest.includes('--start-dir') ? rest[rest.indexOf('--start-dir') + 1] : data.turns.find((r) => r.dir_n)?.dir_n ?? 0);
  let counter = startDir;
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
const csvOut = rest.includes('--csv') ? rest[rest.indexOf('--csv') + 1] : null;

const median = (xs) => {
  const v = xs.filter((x) => typeof x === 'number' && Number.isFinite(x)).sort((a, b) => a - b);
  return v.length ? Math.round((v[Math.floor((v.length - 1) / 2)] + v[Math.ceil((v.length - 1) / 2)]) / 2) : null;
};
const at = (row, stage, nth = 0) => {
  const hits = row.stages.filter(([name]) => name === stage);
  return hits[nth]?.[1] ?? null;
};
const lastAt = (row, stage) => { const hits = row.stages.filter(([name]) => name === stage); return hits.length ? hits[hits.length - 1][1] : null; };
const diff = (a, b) => (a !== null && b !== null && a !== undefined && b !== undefined ? b - a : null);

function breakdown(row) {
  const headless = !row.status_trace && row.first_frame_ms === undefined;
  // Only stages after this turn's own creation belong to it (the worker's stage log spans turns).
  const own = { ...row, stages: row.stages.filter(([, t]) => t >= (row.create_off_ms ?? 0) - 1) };
  const fin = row.finished_off_ms;
  const out = { op_create: row.create_off_ms };
  const spawning = at(own, 'spawning_pane');
  const waitingReady = at(own, 'waiting_for_pane_readiness');
  const waitingInput = at(own, 'waiting_input');
  const dispatching = lastAt(own, 'dispatching');
  const dispatched = lastAt(own, 'dispatched_waiting_for_task_completion');
  if (headless) {
    const running = at(own, 'process_running');
    const first = at(own, 'first_output');
    out.spawn = diff(at(own, 'dispatching'), running);
    out.cli_startup_to_first_output = diff(running, first);
    out.first_output_to_completed = diff(first, fin);
  } else {
    out.pane_spawn = diff(spawning, waitingReady);
    if (waitingInput !== null && dispatching !== null && waitingInput < dispatching) {
      out.tui_ready = diff(waitingReady, waitingInput);
      out.trust_to_dispatch = diff(waitingInput, dispatching);
    } else {
      out.tui_ready = diff(waitingReady, dispatching);
    }
    out.delivery = diff(dispatching, dispatched);
    out.model = diff(dispatched, row.reply_visible_ms);
    out.stream_tail = diff(row.reply_visible_ms, row.screen_final_ms);
    out.completion = row.screen_final_to_finished_ms;
  }
  out.return = row.return_lag_ms;
  out.total = row.wall_total_ms;
  out.finish = fin;
  return out;
}

const groups = new Map();
for (const row of data.turns) {
  if (row.status !== 'completed') continue;
  const key = `${row.scenario}|${row.kind}`;
  if (!groups.has(key)) groups.set(key, []);
  groups.get(key).push(breakdown(row));
}
const keys = [...new Set([...groups.values()].flatMap((rows) => rows.flatMap((r) => Object.keys(r))))];
const table = [];
for (const [key, rows] of groups) {
  const [scenario, kind] = key.split('|');
  const entry = { scenario, kind, n: rows.length };
  for (const k of keys) entry[k] = median(rows.map((r) => r[k]));
  table.push(entry);
}
const failed = data.turns.filter((r) => r.status !== 'completed').map((r) => `${r.scenario}/${r.kind}: ${r.status} ${r.error ?? ''}`.slice(0, 120));
if (md) {
  const cols = ['scenario', 'kind', 'n', ...keys];
  console.log(`| ${cols.join(' | ')} |\n|${cols.map(() => '---').join('|')}|`);
  for (const e of table) console.log(`| ${cols.map((c) => e[c] ?? '').join(' | ')} |`);
} else {
  console.table(table);
}
if (failed.length) console.log('not completed:', failed.length, [...new Set(failed)].join('\n  '));
if (csvOut) writeFileSync(csvOut, [['scenario', 'kind', 'n', ...keys].join(','), ...table.map((e) => ['scenario', 'kind', 'n', ...keys].map((c) => e[c] ?? '').join(','))].join('\n'));
