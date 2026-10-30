#!/usr/bin/env node
// Catches in a second what a merge of two feature branches otherwise shows after a 2-minute compile:
// conflict markers left in a file, and the same line twice in the files every feature adds a line to
// (module lists, the command list, the end-to-end test list, doc section numbers).
//
//   node scripts/check-merge-leftovers.mjs
//
// Run by `scripts/check.mjs`. Exit code 1 and one line per problem when something is found.

import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const tauri = 'packages/app/src-tauri';
const problems = [];

const read = (rel) => {
  try {
    return readFileSync(join(root, rel), 'utf8').split(/\r?\n/);
  } catch {
    return null; // a file that does not exist in this checkout is not a problem
  }
};

// 1. Conflict markers, in the places the code and the docs live.
const tracked = execFileSync('git', ['ls-files', '-z', 'docs', 'packages/app/src', `${tauri}/src`, `${tauri}/tests`, '.gitattributes'], {
  cwd: root,
  maxBuffer: 1 << 28,
})
  .toString('utf8')
  .split('\0')
  .filter((f) => f && !/\.(png|jpg|jpeg|gif|ico|icns|woff2?|cast|zst|zip|mp4)$/i.test(f));
for (const rel of tracked) {
  const lines = read(rel);
  if (!lines) continue;
  lines.forEach((line, i) => {
    if (/^(<<<<<<<|>>>>>>>) /.test(line)) problems.push(`${rel}:${i + 1}  conflict marker`);
  });
}

// 2. The same line twice in a list that every feature appends to.
function duplicateLines(rel, pattern, what) {
  const lines = read(rel);
  if (!lines) return;
  const seen = new Map();
  lines.forEach((line, i) => {
    if (!pattern.test(line)) return;
    const key = line.trim();
    if (seen.has(key)) problems.push(`${rel}:${i + 1}  ${what} listed twice (first at line ${seen.get(key)}): ${key}`);
    else seen.set(key, i + 1);
  });
}
duplicateLines(`${tauri}/src/commands/mod.rs`, /^(pub )?mod /, 'module');
duplicateLines(`${tauri}/src/commands/mod.rs`, /^pub use /, 're-export');
duplicateLines(`${tauri}/src/lib.rs`, /^\s+commands::[a-z_0-9]+,\s*$/, 'command');
duplicateLines(`${tauri}/tests/fake_tui_e2e.rs`, /^mod /, 'end-to-end module');
duplicateLines(`${tauri}/tests/fake_tui_e2e.rs`, /^#\[path/, 'end-to-end path');
duplicateLines(`${tauri}/src/fake_tui/mod.rs`, /^(pub )?mod /, 'fake-agent module');

// 3. Doc sections numbered twice ("## 3.", "### 2.1").
for (const rel of ['docs/FEATURES.md', 'docs/TEST-PLAN.md', 'docs/REPO_MAP.md']) {
  const lines = read(rel);
  if (!lines) continue;
  const seen = new Map();
  lines.forEach((line, i) => {
    const m = line.match(/^(#{2,3}) ([0-9]+[a-z]?(?:\.[0-9]+[a-z]?)?) /);
    if (!m) return;
    const key = `${m[1]} ${m[2]}`;
    if (seen.has(key)) problems.push(`${rel}:${i + 1}  section number ${m[2]} used twice (first at line ${seen.get(key)})`);
    else seen.set(key, i + 1);
  });
}

if (problems.length === 0) {
  console.log('check-merge-leftovers: no conflict markers or duplicated list lines.');
  process.exit(0);
}
console.error('check-merge-leftovers: found leftovers from a merge.\n');
for (const p of problems) console.error(`  ${p}`);
process.exit(1);
