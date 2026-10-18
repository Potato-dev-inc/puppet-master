#!/usr/bin/env node
// Privacy guard: fail if a tracked file contains a real-looking Windows/macOS
// user folder (`C:\Users\<name>\`, `C:/Users/<name>/`, `C:\\Users\\<name>\\`,
// `/c/Users/<name>/`, `/Users/<name>/`) or a consumer-mailbox address.
//
// The repo is public, so docs, fixtures and tests must use a placeholder name.
// Allowed placeholder names are listed below; add to ALLOWED_NAMES only for an
// obviously fake name (never a real person's folder).
//
//   node scripts/check-no-personal-paths.mjs
//
// Run by the `What changed` job in .github/workflows/ci.yml.

import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

// Compared case-insensitively. `you-pc` and `You Pc` (matched as `you`) are the
// same-length stand-ins used in terminal-screen fixtures where column widths matter.
const ALLOWED_NAMES = new Set([
  // Documented placeholders.
  'you', 'you-pc', 'me', 'runneradmin', 'name', 'username', 'user', 'yourname',
  // Obviously fake names already used by unit tests and docs.
  'dev', 'someone', 'someoneelse', 'ana', 'anabel', 'a', 'x', 'm', 'test', 'example',
  'public', 'default', 'default user', 'all users',
]);

// Local parts allowed on consumer mail domains (e.g. an example address in docs).
const ALLOWED_MAIL_LOCALS = new Set(['you', 'me', 'user', 'example', 'test', 'noreply', 'no-reply']);

// Sources the check does not read: this script and lockfiles/vendored output.
const SKIP = [/(^|\/)package-lock\.json$/, /(^|\/)Cargo\.lock$/, /(^|\/)node_modules\//, /^scripts\/check-no-personal-paths\.mjs$/];

// A `Users` path segment (either slash direction, doubled when escaped in a
// string literal) followed by the folder name. The name stops at a separator,
// whitespace, quote, or common punctuation.
const USER_DIR = /(?<![A-Za-z0-9_])Users(?:\\+|\/+)([^\\/\s'"`<>|*?:;,)\]}]+)/gi;
const MAIL = /[A-Za-z0-9._%+-]+@(?:gmail|googlemail|outlook|hotmail|live|yahoo|icloud|proton|protonmail)\.(?:com|me)\b/gi;

function isPlaceholderShape(name) {
  // %USERNAME%, $env:USERNAME, ${USER}, <name>, [name], wildcard, ~ and similar.
  return /[%$<>{}*\[~]/.test(name);
}

const files = execFileSync('git', ['ls-files', '-z'], { cwd: repoRoot, maxBuffer: 1 << 28 })
  .toString('utf8')
  .split('\0')
  .filter(Boolean)
  .filter((f) => !SKIP.some((re) => re.test(f)));

const problems = [];

for (const file of files) {
  let buf;
  try {
    buf = readFileSync(resolve(repoRoot, file));
  } catch {
    continue; // deleted in the working tree, or a submodule
  }
  if (buf.subarray(0, 8192).includes(0)) continue; // binary
  const lines = buf.toString('utf8').split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    for (const m of line.matchAll(USER_DIR)) {
      const name = m[1];
      if (isPlaceholderShape(name) || ALLOWED_NAMES.has(name.toLowerCase())) continue;
      problems.push({ file, line: i + 1, kind: 'user folder', text: `Users/${name[0]}${'*'.repeat(Math.max(0, name.length - 1))}` });
    }
    for (const m of line.matchAll(MAIL)) {
      const local = m[0].split('@')[0].toLowerCase();
      if (ALLOWED_MAIL_LOCALS.has(local)) continue;
      problems.push({ file, line: i + 1, kind: 'email address', text: `${'*'.repeat(local.length)}@${m[0].split('@')[1]}` });
    }
  }
}

if (problems.length === 0) {
  console.log(`check-no-personal-paths: ${files.length} tracked files clean.`);
  process.exit(0);
}

console.error('check-no-personal-paths: personal data found in tracked files.\n');
// The offending value is masked so CI logs of this public repo do not repeat it.
for (const p of problems) console.error(`  ${p.file}:${p.line}  ${p.kind}: ${p.text}`);
console.error(
  '\nReplace the user folder with the placeholder `C:\\Users\\you\\` (same-length `you-pc` inside' +
    '\nterminal-screen fixtures), and any email with `you@example.com`. If the name is an obviously' +
    '\nfake test value, add it to ALLOWED_NAMES in scripts/check-no-personal-paths.mjs.',
);
process.exit(1);
