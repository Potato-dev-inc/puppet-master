#!/usr/bin/env node
// Renders src/index.template.html into index.html using site.config.json.
// The product name is ONE value: "name" in site.config.json. Change it, run `node site/build.mjs`, done.
// No dependencies. Every {{KEY}} in the template must have a value in the config (or the build fails),
// and every config key is escaped for HTML.
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const cfg = JSON.parse(readFileSync(path.join(here, 'site.config.json'), 'utf8'));
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const vars = Object.fromEntries(Object.entries(cfg).map(([k, v]) => [k.toUpperCase(), esc(v)]));

// Small macros so the template stays readable:
//   [[dots:3:run:]]        a 3x3 dot grid with a class (run | needs | none) and the indices that are "on"
//   [[dots:3::0,2,4,6,8]]  a static pattern
//   [[mark]]               the 3x3 mark used in the nav and the footer
const dots = (n, cls, on) => {
  const onSet = new Set(on.split(',').filter(Boolean).map(Number));
  const cells = [];
  for (let i = 0; i < n * n; i++) {
    const r = Math.floor(i / n);
    const c = i % n;
    cells.push(`<span${onSet.has(i) ? ' class="on"' : ''} style="--r:${r};--c:${c}"></span>`);
  }
  return `<span class="dots ${cls}" style="--n:${n}" aria-hidden="true">${cells.join('')}</span>`;
};
const MARK =
  '<svg viewBox="0 0 16 16" aria-hidden="true" fill="currentColor">' +
  [0, 1, 2].flatMap((r) => [0, 1, 2].map((c) => {
    const centre = r === 1 && c === 1;
    return `<circle cx="${(2.4 + 5.6 * c).toFixed(1)}" cy="${(2.4 + 5.6 * r).toFixed(1)}" r="${centre ? 2.1 : 2}"${centre ? '' : ' opacity=".4"'}/>`;
  })).join('') +
  '</svg>';

const tplRaw = readFileSync(path.join(here, 'src/index.template.html'), 'utf8');
const tpl = tplRaw
  .replace(/\[\[dots:(\d):([\w ]*):([\d,]*)\]\]/g, (_m, n, cls, on) => dots(Number(n), cls, on))
  .replace(/\[\[mark\]\]/g, MARK);
const missing = new Set();
const out = tpl.replace(/\{\{([A-Z_]+)\}\}/g, (m, k) => {
  if (k in vars) return vars[k];
  missing.add(k);
  return m;
});
if (missing.size) {
  console.error(`site.config.json has no value for: ${[...missing].join(', ')}`);
  process.exit(1);
}
writeFileSync(path.join(here, 'index.html'), out);
console.log(`index.html written for "${cfg.name}" (${out.length} bytes)`);
