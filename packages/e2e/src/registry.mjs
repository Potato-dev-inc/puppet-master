// Finds every test file (tests/<area>/<name>.test.mjs), validates its `meta` and filters by --only.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { TESTS_DIR } from './paths.mjs';

function* walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name.startsWith('_') || e.name.startsWith('.')) continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) yield* walk(p);
    else if (e.name.endsWith('.test.mjs')) yield p;
  }
}

const ID_RE = /^[a-z0-9][a-z0-9-]*(\.[a-z0-9][a-z0-9-]*)+$/;

/** Load all tests: [{ id, title, area, tags, edge, fresh, timeout, file, run }] sorted by area then id. */
export async function loadTests({ dir = TESTS_DIR, bust = false } = {}) {
  const out = [];
  const problems = [];
  if (!fs.existsSync(dir)) return { tests: out, problems };
  for (const file of walk(dir)) {
    try {
      const mod = await import(pathToFileURL(file).href + (bust ? `?v=${fs.statSync(file).mtimeMs}` : ''));
      const m = mod.meta;
      if (!m || typeof mod.run !== 'function') throw new Error('must export `meta` and `run(t)`');
      if (!ID_RE.test(m.id || '')) throw new Error(`meta.id "${m.id}" must look like area.name (lowercase, dots and dashes)`);
      if (!m.title) throw new Error('meta.title is required');
      const area = m.area || m.id.split('.')[0];
      if (m.id.split('.')[0] !== area) throw new Error(`meta.id "${m.id}" must start with its area "${area}."`);
      out.push({
        id: m.id, title: m.title, area, tags: m.tags || [], edge: !!m.edge, fresh: !!m.fresh, seed: m.seed || null, skip: typeof m.skip === 'string' && m.skip ? m.skip : (m.skip ? 'skipped' : null), timeout: m.timeout || 60000,
        features: m.features || [], file: path.relative(dir, file).split(path.sep).join('/'), run: mod.run, setup: mod.setup,
      });
    } catch (e) {
      problems.push({ file: path.relative(dir, file), error: e.message });
    }
  }
  const seen = new Map();
  for (const t of out) {
    if (seen.has(t.id)) problems.push({ file: t.file, error: `duplicate id ${t.id} (also in ${seen.get(t.id)})` });
    seen.set(t.id, t.file);
  }
  out.sort((a, b) => a.area.localeCompare(b.area) || a.id.localeCompare(b.id));
  return { tests: out, problems };
}

const globToRe = (g) => new RegExp('^' + g.split('*').map((s) => s.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('.*') + '$');

/**
 * Pick tests. Each selector is one of: a test id, a glob over ids ("chat.*"), an area ("chat"),
 * or "tag:<tag>". Several selectors can be joined with commas; no selector means every test.
 */
export function select(tests, selectors) {
  const sels = (Array.isArray(selectors) ? selectors : [selectors]).flatMap((s) => String(s || '').split(',')).map((s) => s.trim()).filter(Boolean);
  if (!sels.length) return tests;
  const picked = new Set();
  const unmatched = [];
  for (const s of sels) {
    let hits;
    if (s.startsWith('tag:')) hits = tests.filter((t) => t.tags.includes(s.slice(4)));
    else if (s.includes('*')) { const re = globToRe(s); hits = tests.filter((t) => re.test(t.id)); }
    else hits = tests.filter((t) => t.id === s || t.area === s);
    if (!hits.length) unmatched.push(s);
    hits.forEach((t) => picked.add(t));
  }
  const list = tests.filter((t) => picked.has(t));
  list.unmatched = unmatched;
  return list;
}
