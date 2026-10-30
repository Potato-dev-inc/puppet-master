#!/usr/bin/env node
// Run only the tests your change can reach, then stop. The quick loop while you work; run
// `node scripts/check.mjs` once before you push.
//
//   node scripts/test-changed.mjs                 files changed against origin/main (committed, staged, unstaged, new)
//   node scripts/test-changed.mjs --base HEAD     only what is not committed yet
//   node scripts/test-changed.mjs --dry-run       print the plan, run nothing
//   node scripts/test-changed.mjs --files a.rs b.tsx   pretend exactly these files changed
//   node scripts/test-changed.mjs --e2e           also the fake-agent end-to-end tests that match
//   node scripts/test-changed.mjs --no-dependents Rust: the changed modules only, not the ones that use them
//
// How a file maps to tests (a heuristic, not a proof: it can miss a far-away dependent, which is why
// `check.mjs` and CI still run everything):
//   Rust  src/<mod>/...      -> `cargo test --lib <mod>::`, plus every top-level module that mentions `crate::<mod>`
//         src/lib.rs, Cargo.toml, Cargo.lock, build.rs, tauri.conf.json, integrations/**, shared fixtures
//                            -> all library tests
//         src/mcp_shim/**, src/bin/**   -> `cargo test --bin puppet-master-mcp`
//         tests/fake_tui_*.rs, src/fake_tui/**   -> the end-to-end target (only with --e2e)
//   TS    *.ts / *.tsx under packages/app   -> `vitest related` (follows imports), and tsc
//         other files under packages/app/src (css, json)  -> the tests that name the file
//         packages/shared/** -> shared build, tsc, and all of vitest (the app imports it)
//         packages/cli/**    -> the CLI's own tests
//   docs, markdown: nothing.

import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import {
  appDir,
  cargoEnv,
  checkoutName,
  fmt,
  git,
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
const has = (f) => args.includes(f);
const value = (f) => (args.includes(f) ? args[args.indexOf(f) + 1] : undefined);
const dry = has('--dry-run');
const withE2e = has('--e2e');
const noDependents = has('--no-dependents');
// A pile of changed modules is no longer "a few tests": above this many, run the whole library suite.
const MAX_MODULES = Number(value('--max-modules') ?? 14);

// ---- which files changed ----------------------------------------------------------------------------------

function changedFiles() {
  const filesArg = args.indexOf('--files');
  if (filesArg >= 0) return args.slice(filesArg + 1).filter((a) => !a.startsWith('--')).map((f) => f.replace(/\\/g, '/'));
  const base = value('--base') ?? defaultBase();
  const lines = new Set();
  const add = (out) => (out ?? '').split('\n').map((l) => l.trim()).filter(Boolean).forEach((l) => lines.add(l.replace(/\\/g, '/')));
  add(git(['diff', '--name-only', '--diff-filter=ACMRD', base]));
  add(git(['ls-files', '--others', '--exclude-standard']));
  return [...lines];
}

function defaultBase() {
  for (const ref of ['origin/main', 'main']) {
    const mb = git(['merge-base', 'HEAD', ref]);
    if (mb) return mb.trim();
  }
  return 'HEAD';
}

// ---- classify ---------------------------------------------------------------------------------------------

const TAURI = 'packages/app/src-tauri/';
const plan = {
  rustModules: new Set(),
  rustAll: false,
  mcpBin: false,
  e2e: false,
  tsFiles: [],
  otherAppFiles: [],
  sharedChanged: false,
  cliChanged: false,
  notes: [],
};

const RUST_ALL = [
  /^packages\/app\/src-tauri\/(Cargo\.(toml|lock)|build\.rs|tauri\.conf\.json|capabilities\/)/,
  /^packages\/app\/src-tauri\/src\/(lib|main)\.rs$/,
  /^packages\/app\/src-tauri\/(agent-definitions|catalog|resources)\//,
  /^integrations\//,
  /^packages\/shared\/fixtures\//,
  /^packages\/app\/src\/components\/simple\/folder-cases\.json$/,
  /^relay\//,
  /^\.github\/workflows\//,
];

for (const f of changedFiles()) {
  if (RUST_ALL.some((re) => re.test(f))) {
    plan.rustAll = true;
    plan.notes.push(`${f}: read by many Rust tests -> all library tests`);
    continue;
  }
  if (f.startsWith(`${TAURI}src/`)) {
    const rest = f.slice(`${TAURI}src/`.length);
    if (rest.startsWith('mcp_shim/') || rest.startsWith('bin/')) {
      plan.mcpBin = true;
      continue;
    }
    if (rest.startsWith('fake_tui/')) {
      plan.e2e = true;
      continue;
    }
    if (!rest.endsWith('.rs')) {
      // include_str! fixtures and the like live next to the module that reads them.
      const top = rest.split('/')[0];
      if (rest.includes('/')) plan.rustModules.add(top);
      else plan.rustAll = true;
      continue;
    }
    const top = rest.includes('/') ? rest.split('/')[0] : rest.replace(/\.rs$/, '');
    plan.rustModules.add(top);
    continue;
  }
  if (f.startsWith(`${TAURI}tests/`)) {
    if (/fake_tui|what_broke_it/.test(f)) plan.e2e = true;
    else plan.rustAll = true;
    continue;
  }
  if (f.startsWith('packages/app/')) {
    const rel = f.slice('packages/app/'.length);
    if (/\.(ts|tsx)$/.test(rel)) plan.tsFiles.push(rel);
    else if (rel.startsWith('src/')) plan.otherAppFiles.push(rel);
    else if (/^(package\.json|vite\.config|vitest\.config|tsconfig|postcss|tailwind)/.test(rel)) {
      plan.sharedChanged = true; // a tooling change can move any test
      plan.notes.push(`${f}: tooling -> all of vitest and tsc`);
    }
    continue;
  }
  if (f.startsWith('packages/shared/')) plan.sharedChanged = true;
  else if (f.startsWith('packages/cli/')) plan.cliChanged = true;
  else if (f === 'package.json' || f === 'package-lock.json' || f === 'tsconfig.base.json') {
    plan.sharedChanged = true;
    plan.notes.push(`${f}: tooling -> all of vitest and tsc`);
  }
  // everything else (docs, markdown, scripts, site): no test reads it
}

// ---- Rust: add the modules that use the changed ones ------------------------------------------------------

function walkRs(dir, out = []) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) walkRs(p, out);
    else if (e.name.endsWith('.rs')) out.push(p);
  }
  return out;
}

function dependentsOf(changed) {
  const src = join(tauriDir, 'src');
  const users = new Map(); // top-level module -> set of top-level modules it mentions
  for (const file of walkRs(src)) {
    const rel = relative(src, file).replace(/\\/g, '/');
    const top = rel.includes('/') ? rel.split('/')[0] : rel.replace(/\.rs$/, '');
    const text = readFileSync(file, 'utf8');
    for (const m of text.matchAll(/crate::([a-z_][a-z0-9_]*)/g)) {
      if (!users.has(m[1])) users.set(m[1], new Set());
      users.get(m[1]).add(top);
    }
  }
  const out = new Set(changed);
  for (const c of changed) for (const u of users.get(c) ?? []) out.add(u);
  return out;
}

let rustFilters = [];
if (!plan.rustAll && plan.rustModules.size) {
  const modules = noDependents ? plan.rustModules : dependentsOf([...plan.rustModules]);
  if (modules.size > MAX_MODULES) {
    plan.rustAll = true;
    plan.notes.push(`${plan.rustModules.size} changed module(s) are used by ${modules.size} modules in all -> all library tests`);
  } else {
    rustFilters = [...modules].sort().map((m) => `${m}::`);
  }
}

// ---- show the plan ----------------------------------------------------------------------------------------

const steps = [];
const logs = dry ? '' : logDir(checkoutName());

if (plan.rustAll) steps.push({ label: 'rust  all library tests', kind: 'cargo', args: ['test', '--lib'] });
else if (rustFilters.length) {
  steps.push({ label: `rust  ${rustFilters.length} module(s): ${rustFilters.join(' ')}`, kind: 'cargo', args: ['test', '--lib', '--', ...rustFilters] });
}
if (plan.mcpBin) steps.push({ label: 'rust  puppet-master-mcp tests', kind: 'cargo', args: ['test', '--bin', 'puppet-master-mcp'] });
if (plan.e2e) {
  if (withE2e) {
    steps.push({
      label: 'rust  fake-agent end-to-end tests (all; narrow with: cargo test --features dev-fake-tui --test fake_tui_e2e <name>)',
      kind: 'cargo',
      args: ['test', '--features', 'dev-fake-tui', '--test', 'fake_tui_e2e'],
    });
  } else {
    plan.notes.push('end-to-end tests touched: not run (add --e2e, or `npm run test:fake-tui`)');
  }
}

const needTsc = plan.tsFiles.length > 0 || plan.sharedChanged || plan.otherAppFiles.length > 0;
if (plan.sharedChanged) {
  steps.push({ label: 'js    vitest (all)', kind: 'app', cmd: npx, args: ['vitest', 'run'] });
} else if (plan.tsFiles.length) {
  steps.push({ label: `js    vitest related ${plan.tsFiles.length} file(s)`, kind: 'app', cmd: npx, args: ['vitest', 'related', '--run', ...plan.tsFiles] });
}
if (!plan.sharedChanged && plan.otherAppFiles.length) {
  const tests = new Set();
  for (const f of plan.otherAppFiles) {
    const base = f.split('/').pop();
    const hit = git(['grep', '-l', '-F', base, '--', 'packages/app/src/**/*.test.ts', 'packages/app/src/**/*.test.tsx']);
    for (const t of (hit ?? '').split('\n').filter(Boolean)) tests.add(t.replace(/^packages\/app\//, ''));
  }
  if (tests.size) steps.push({ label: `js    vitest ${tests.size} test file(s) naming a changed css/json file`, kind: 'app', cmd: npx, args: ['vitest', 'run', ...tests] });
}
if (needTsc) {
  steps.push({ label: 'js    tsc app + service worker', kind: 'tsc' });
}
if (plan.cliChanged) steps.push({ label: 'js    cli tests', kind: 'root', cmd: npm, args: ['test', '--workspace=@puppet-master/cli'] });

console.log(`changed: ${plan.rustModules.size} Rust module(s), ${plan.tsFiles.length} TS file(s), ${plan.otherAppFiles.length} other app file(s)${plan.sharedChanged ? ', shared/tooling' : ''}`);
for (const n of plan.notes) console.log(`  note: ${n}`);
if (steps.length === 0) {
  console.log('nothing to run: no changed file has tests behind it.');
  process.exit(0);
}
for (const s of steps) console.log(`  plan: ${s.label}`);
if (dry) process.exit(0);

// ---- run it -----------------------------------------------------------------------------------------------

const t0 = Date.now();
try {
  const did = prepare({});
  if (did.length) console.error(`[test-changed] prepared: ${did.join(', ')}`);
} catch (e) {
  console.error(`[test-changed] ${e.message}`);
  process.exit(1);
}

const cargoSteps = steps.filter((s) => s.kind === 'cargo');
const jsSteps = steps.filter((s) => s.kind !== 'cargo');
let n = 0;
const runStep = (s) => {
  const logPath = join(logs, `changed-${++n}.log`);
  if (s.kind === 'cargo') return runLogged(s.label, 'cargo', s.args, { cwd: tauriDir, env: cargoEnv(), logPath });
  if (s.kind === 'tsc') {
    return runLogged(s.label, npm, ['run', 'typecheck'], { cwd: appDir, logPath });
  }
  return runLogged(s.label, s.cmd, s.args, { cwd: s.kind === 'app' ? appDir : repoRoot, logPath });
};

// Cargo steps share one build directory, so they go one after the other; the JS steps run beside them.
const lanes = [
  (async () => {
    const out = [];
    for (const s of cargoSteps) out.push(await runStep(s));
    return out;
  })(),
  (async () => {
    const out = [];
    for (const s of jsSteps) out.push(await runStep(s));
    return out;
  })(),
];
const results = (await Promise.all(lanes)).flat();

let failed = 0;
console.log('');
for (const r of results) {
  const ok = r.code === 0;
  if (!ok) failed++;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${fmt(r.seconds).padStart(7)}  ${r.label}`);
  const lines = r.lines();
  const keep = ok
    ? summarize(lines, [/^test result:/, /Test Files /, /^\s*Tests /])
    : summarize(lines, [/^error/, /^test .* FAILED/, /panicked at/, /FAIL /, /error TS\d+/]);
  for (const l of keep) console.log(`          ${l.trim().slice(0, 200)}`);
  if (!ok) console.log(`          log: ${r.log}`);
}
console.log(`\n${failed === 0 ? 'CHANGED-TESTS PASSED' : `${failed} STEP(S) FAILED`} in ${fmt((Date.now() - t0) / 1000)}. Not everything ran: use scripts/check.mjs before you push.`);
process.exit(failed === 0 ? 0 : 1);
