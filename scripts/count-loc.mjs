#!/usr/bin/env node
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');

const SKIP_DIRS = new Set([
  '.git',
  'node_modules',
  'dist',
  'dev-dist',
  'target',
  'pwa-dist',
]);

const EXT_TO_LANG = {
  '.ts': 'TypeScript',
  '.tsx': 'TypeScript',
  '.js': 'JavaScript',
  '.jsx': 'JavaScript',
  '.mjs': 'JavaScript',
  '.cjs': 'JavaScript',
  '.rs': 'Rust',
};

function walk(dir, onFile) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue;
      walk(join(dir, entry.name), onFile);
      continue;
    }
    if (!entry.isFile()) continue;
    onFile(join(dir, entry.name));
  }
}

function countLines(text) {
  if (text.length === 0) return 0;
  const lines = text.split(/\r?\n/);
  if (lines.at(-1) === '') lines.pop();
  return lines.length;
}

const totals = Object.fromEntries(
  [...new Set(Object.values(EXT_TO_LANG))].map((lang) => [lang, { files: 0, lines: 0 }]),
);

function isSourceFile(filePath) {
  const base = filePath.slice(filePath.lastIndexOf('/') + 1);
  if (/\.bundle\.(js|cjs|mjs)$/i.test(base)) return false;
  return true;
}

walk(repoRoot, (filePath) => {
  if (!isSourceFile(filePath)) return;

  const ext = extname(filePath);
  const lang = EXT_TO_LANG[ext];
  if (!lang) return;

  const lines = countLines(readFileSync(filePath, 'utf8'));
  totals[lang].files += 1;
  totals[lang].lines += lines;
});

const grandTotal = Object.values(totals).reduce(
  (sum, { files, lines }) => ({ files: sum.files + files, lines: sum.lines + lines }),
  { files: 0, lines: 0 },
);

console.log(`Lines of code in ${repoRoot}\n`);
console.log('Language       Files    Lines');
console.log('-----------------------------');

for (const lang of ['TypeScript', 'JavaScript', 'Rust']) {
  const { files, lines } = totals[lang];
  console.log(`${lang.padEnd(13)}${String(files).padStart(6)}${String(lines).padStart(9)}`);
}

console.log('-----------------------------');
console.log(`${'Total'.padEnd(13)}${String(grandTotal.files).padStart(6)}${String(grandTotal.lines).padStart(9)}`);
