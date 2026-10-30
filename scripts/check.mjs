#!/usr/bin/env node
// One command for "is my change fine?": the checks CI runs, run side by side so the whole thing takes as
// long as the slowest one (the Rust build) and no longer.
//
//   node scripts/check.mjs                 everything (Rust lib tests, tsc x2, vitest, repo guards)
//   node scripts/check.mjs --rust-only     only the Rust lane
//   node scripts/check.mjs --js-only       only the TypeScript lane
//   node scripts/check.mjs --filter tui::  Rust tests whose name contains this (repeatable); the JS lane still runs
//   node scripts/check.mjs --install       run `npm ci` first
//
// Rust lane: `cargo test --lib` (the library unit tests; the fake-agent end-to-end tests need
// `npm run test:fake-tui` or `scripts/test-changed.mjs --e2e`). JS lane: `tsc --noEmit` for the app and the
// service worker (incremental: ~5 s when nothing changed), then `vitest run`. Guards: no personal paths in tracked files, no merge leftovers.
//
// Logs go to <tmp>/pm-check/<checkout name>/ (override with PM_CHECK_LOGS); the terminal gets one line per
// lane and the first failing lines. Exit code 0 only when every lane passed.
//
// Cargo settings are yours: set CARGO_TARGET_DIR to keep a worktree's build off the repo drive, and do not
// flip CARGO_INCREMENTAL / CARGO_PROFILE_*_DEBUG between runs (a changed setting rebuilds the whole crate).

import { join } from 'node:path';
import {
  appDir,
  cargoEnv,
  checkoutName,
  fmt,
  logDir,
  npm,
  npx,
  prepare,
  repoRoot,
  runLogged,
  summarize,
  tauriDir,
} from './dev-lib.mjs';

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const filters = [];
for (let i = 0; i < args.length; i++) if (args[i] === '--filter' && args[i + 1]) filters.push(args[++i]);
const rustOnly = flag('--rust-only');
const jsOnly = flag('--js-only');

const logs = logDir(checkoutName());
const started = Date.now();

try {
  const did = prepare({ install: flag('--install') });
  if (did.length) console.error(`[check] prepared: ${did.join(', ')}`);
} catch (e) {
  console.error(`[check] ${e.message}`);
  process.exit(1);
}

const lanes = [];

if (!jsOnly) {
  lanes.push(
    runLogged('rust  cargo test --lib', 'cargo', ['test', '--lib', ...(filters.length ? ['--', ...filters] : [])], {
      cwd: tauriDir,
      env: cargoEnv(),
      logPath: join(logs, 'rust.log'),
    }),
  );
}

if (!rustOnly) {
  // The TypeScript lane runs in order (vitest is the long one and starts last), beside the Rust build.
  lanes.push(
    (async () => {
      const t0 = Date.now();
      const parts = [];
      for (const [label, cmd, a, log] of [
        // The app's own `typecheck` script: app and service worker, with the incremental cache (see package.json).
        ['tsc app + service worker', npm, ['run', 'typecheck'], 'tsc.log'],
        ['vitest', npx, ['vitest', 'run'], 'vitest.log'],
      ]) {
        const r = await runLogged(label, cmd, a, { cwd: appDir, logPath: join(logs, log) });
        parts.push(r);
        if (r.code !== 0) break;
      }
      const bad = parts.find((p) => p.code !== 0);
      const last = parts[parts.length - 1];
      return {
        label: `js    ${parts.map((p) => p.label).join(' + ')}`,
        code: bad ? bad.code : 0,
        seconds: (Date.now() - t0) / 1000,
        log: last.log,
        lines: last.lines,
      };
    })(),
  );
}

// Fast guards, in the same process.
const guards = [
  ['guard personal paths', 'node', [join(repoRoot, 'scripts', 'check-no-personal-paths.mjs')], 'guard-paths.log'],
  ['guard merge leftovers', 'node', [join(repoRoot, 'scripts', 'check-merge-leftovers.mjs')], 'guard-merge.log'],
];
for (const [label, cmd, a, log] of guards) lanes.push(runLogged(label, cmd, a, { cwd: repoRoot, logPath: join(logs, log) }));

const results = await Promise.all(lanes);

let failed = 0;
console.log('');
for (const r of results) {
  const ok = r.code === 0;
  if (!ok) failed++;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${fmt(r.seconds).padStart(7)}  ${r.label}`);
  const lines = r.lines();
  const interesting = ok
    ? summarize(lines, [/^test result:/, /Test Files /, /^\s*Tests /, /clean\./])
    : summarize(lines, [/^error/, /^test .* FAILED/, /panicked at/, /FAIL /, /error TS\d+/, /^\s+\S+:\d+/, /problems|found in tracked|leftover/i]);
  for (const l of interesting) console.log(`          ${l.trim().slice(0, 200)}`);
  if (!ok) console.log(`          log: ${r.log}`);
}
console.log(`\n${failed === 0 ? 'ALL CHECKS PASSED' : `${failed} CHECK(S) FAILED`} in ${fmt((Date.now() - started) / 1000)}  (logs: ${logs})`);
process.exit(failed === 0 ? 0 : 1);
