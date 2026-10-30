#!/usr/bin/env node
// "Where does X live?" in under a second, without reading the 220 KB repository map.
//
//   node scripts/where.mjs approval prompt          rows of docs/REPO_MAP.md and docs/FEATURES.md that mention all words
//   node scripts/where.mjs -n 20 worktree merge     show 20 rows instead of 8
//   node scripts/where.mjs --files checkpoint       also list tracked file paths containing the word
//
// It ranks the table rows and list items of the maps by how many of your words they contain (every word
// must appear for a row to count as a full match; otherwise the best partial rows are shown) and prints
// each with the heading it sits under. Then open the file it names. The maps are the source: when a row is
// wrong, fix the row.

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const argv = process.argv.slice(2);
let limit = 8;
let withFiles = false;
const words = [];
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === '-n') limit = Number(argv[++i]) || limit;
  else if (argv[i] === '--files') withFiles = true;
  else words.push(argv[i].toLowerCase());
}
if (words.length === 0) {
  console.error('usage: node scripts/where.mjs [-n 8] [--files] <word> [<word> ...]');
  process.exit(2);
}

const SOURCES = ['docs/REPO_MAP.md', 'docs/FEATURES.md'];
const rows = [];
for (const rel of SOURCES) {
  const file = join(root, rel);
  if (!existsSync(file)) continue;
  let heading = '';
  readFileSync(file, 'utf8')
    .split(/\r?\n/)
    .forEach((line, i) => {
      if (/^#{1,4} /.test(line)) heading = line.replace(/^#+ /, '').trim();
      else if (/^\s*(\||[-*] )/.test(line) && !/^\|[-| :]+\|?$/.test(line)) {
        const lower = line.toLowerCase();
        const hits = words.filter((w) => lower.includes(w)).length;
        if (hits > 0) {
          // A word in the first cell (the file or topic the row is about) or in the heading counts for more.
          const first = (line.split('|')[1] ?? line).toLowerCase();
          const head = heading.toLowerCase();
          const score =
            hits * 10 +
            words.filter((w) => first.includes(w)).length * 6 +
            words.filter((w) => head.includes(w)).length * 3 +
            (/where do i change/i.test(heading) ? 5 : 0);
          rows.push({ rel, n: i + 1, heading, line, hits, score, length: line.length });
        }
      }
    });
}

// Best score first; among equals, the shorter row (a precise row beats a long one that mentions everything).
rows.sort((a, b) => b.score - a.score || a.length - b.length);
const full = rows.filter((r) => r.hits === words.length);
const shown = (full.length ? full : rows).slice(0, limit);

if (shown.length === 0) console.log(`no row in ${SOURCES.join(' or ')} mentions: ${words.join(' ')}`);
else if (!full.length && words.length > 1) console.log(`no row has all of: ${words.join(' ')}. Best partial matches:\n`);
for (const r of shown) {
  const text = r.line.replace(/^\|\s*/, '').replace(/\s*\|\s*$/, '').replace(/\s*\|\s*/g, '  =>  ');
  console.log(`${r.rel}:${r.n}  [${r.heading}]\n  ${text.length > 330 ? `${text.slice(0, 330)}...` : text}\n`);
}
if (full.length > shown.length) console.log(`(${full.length - shown.length} more full matches: -n ${full.length})`);

if (withFiles) {
  const files = execFileSync('git', ['ls-files'], { cwd: root, maxBuffer: 1 << 26 }).toString().split('\n');
  const hit = files.filter((f) => words.every((w) => f.toLowerCase().includes(w))).slice(0, 25);
  console.log(hit.length ? `files:\n  ${hit.join('\n  ')}` : 'files: none');
}
