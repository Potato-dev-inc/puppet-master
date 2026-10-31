#!/usr/bin/env node
// npm run e2e -- [--only <id|glob|area|tag:x>[,..]] [--list [--json]] [--no-video] [--keep-app] [--fresh] [--headless]
//
//   npm run e2e -- --list
//   npm run e2e -- --only chat.send-message
//   npm run e2e -- --only "chat.*,settings" --no-video
//   npm run e2e -- --only tag:smoke
// With no --only every test runs. Exit code: 0 all passed, 1 a test failed, 2 could not run.
import { loadTests, select } from './registry.mjs';
import { runTests } from './runner.mjs';
import { RESULTS_DIR, readBuildInfo } from './paths.mjs';
import { reapOrphans } from './launcher.mjs';

const argv = process.argv.slice(2);
const flag = (name) => argv.includes(name);
function values(name) {
  const out = [];
  for (let i = 0; i < argv.length; i++) if (argv[i] === name && argv[i + 1]) out.push(argv[++i]);
  else if (argv[i].startsWith(name + '=')) out.push(argv[i].slice(name.length + 1));
  return out;
}
const value = (name) => values(name)[0];

if (flag('--help') || flag('-h')) {
  console.log(`usage: npm run e2e -- [options] [selector ...]
  --only <sel>     test id, glob (chat.*), area (chat) or tag:<tag>; comma-separated; also accepted without the flag
  --list [--json]  list the tests and exit
  --no-video       do not record videos (faster)
  --keep-app       leave the app running after the run (prints how to stop it)
  --fresh          restart the app before every test (default: one app for the run, reset between tests)
  --headless       place the window off-screen
  --anchor x,y     window anchor (default: PM_WINDOW_ANCHOR or the left monitor)
  --run-id <id>    name of the results folder (default: a timestamp)
  --known-bugs     also run tests marked skip in their meta (known bugs, not yet verified); same as E2E_RUN_KNOWN_BUGS=1
  --events         also print machine-readable "@@E2E {json}" lines (the dashboard uses it)
Results: ${RESULTS_DIR}`);
  process.exit(0);
}

if (flag('--known-bugs')) process.env.E2E_RUN_KNOWN_BUGS = '1';

const known = new Set(['--only', '--run-id', '--anchor']);
const positional = [];
for (let i = 0; i < argv.length; i++) {
  if (known.has(argv[i])) { i++; continue; }
  if (argv[i].startsWith('-')) continue;
  positional.push(argv[i]);
}
const selectors = [...values('--only'), ...positional];

const { tests, problems } = await loadTests();
for (const p of problems) console.error(`! ${p.file}: ${p.error}`);

if (flag('--list')) {
  const picked = selectors.length ? select(tests, selectors) : tests;
  if (flag('--json')) console.log(JSON.stringify(picked.map(({ run, setup, ...t }) => t), null, 1));
  else {
    let area = '';
    for (const t of picked) {
      if (t.area !== area) { area = t.area; console.log(`\n${area}`); }
      console.log(`  ${t.id.padEnd(40)} ${t.title}${t.edge ? '  [edge]' : ''}${t.tags.length ? '  #' + t.tags.join(' #') : ''}`);
    }
    console.log(`\n${picked.length} test(s)`);
  }
  process.exit(problems.length ? 1 : 0);
}

const picked = select(tests, selectors);
if (picked.unmatched?.length) console.error(`no test matches: ${picked.unmatched.join(', ')}`);
if (!picked.length) { console.error('nothing to run (try --list)'); process.exit(2); }
if (!readBuildInfo() && !process.env.E2E_APP_EXE) { console.error('no build found. Run: npm run e2e:build'); process.exit(2); }

const machine = flag('--events');
const signal = { stopped: false };
if (machine) {
  // the dashboard sends "stop" on stdin; a closed stdin also stops the run (the dashboard went away)
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (d) => { if (/stop/.test(d)) signal.stopped = true; });
  process.stdin.on('end', () => { signal.stopped = true; });
}

const icon = { pass: 'PASS', fail: 'FAIL', skip: 'SKIP' };
const emit = (e) => {
  if (machine) console.log('@@E2E ' + JSON.stringify(e));
  switch (e.type) {
    case 'run-start': console.log(`run ${e.runId}: ${e.total} test(s)`); break;
    case 'app-start': console.log(`app started in ${(e.ms / 1000).toFixed(1)} s (window ${e.viewport ? e.viewport.w + 'x' + e.viewport.h : '?'}, pid ${e.pid}, debugging port ${e.cdpPort}, bridge ${e.bridgePort})`); break;
    case 'test-start': console.log(`\n[${e.index}/${e.total}] ${e.id}  ${e.title}`); break;
    case 'step': console.log(`    ${(e.atMs / 1000).toFixed(1).padStart(5)} s  ${e.name}`); break;
    case 'log': console.log(`          ${e.line}`); break;
    case 'test-end': console.log(`  ${icon[e.status] || e.status}  ${e.id}  ${(e.durationMs / 1000).toFixed(1)} s${e.video ? '  ' + e.video : ''}${e.error ? '\n        ' + e.error.split('\n')[0] : ''}`); break;
    case 'app-kept': console.log(`app left running: pid ${e.pid}, debugging port ${e.cdpPort}, bridge ${e.bridgePort}. Stop it with: taskkill /PID ${e.pid} /T /F`); break;
    case 'run-end': console.log(`\n${e.passed} passed, ${e.failed} failed, ${e.skipped} skipped in ${(e.durationMs / 1000).toFixed(1)} s\nresults: ${RESULTS_DIR}/${e.runId}`); break;
    default: break;
  }
};

const reaped = reapOrphans();
if (reaped.length) console.log(`stopped ${reaped.length} app(s) left over from a run that was killed: pid ${reaped.join(', ')}`);

let summary;
try {
  summary = await runTests(picked, {
    video: !flag('--no-video'), keepApp: flag('--keep-app'), fresh: flag('--fresh'), headless: flag('--headless'),
    runId: value('--run-id'), anchor: value('--anchor'), signal,
  }, emit);
} catch (e) {
  console.error('\nrun failed: ' + (e.stack || e.message));
  process.exit(2);
}
process.exit(summary.failed ? 1 : 0);
