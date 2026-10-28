#!/usr/bin/env node
// Checks the publisher of every npm and uv package the Agent Store catalog names.
// Each package must come from the repository its entry.toml [publisher] table records:
//   npm -> `npm view <pkg> repository.url`, compared with publisher.npm_repo
//   uv  -> PyPI JSON API (https://pypi.org/pypi/<pkg>/json), project_urls, compared with publisher.pypi_repo
//
// This asks the registries over the network, so a maintainer runs it before a release. It is not in CI.
//
//   node scripts/store-verify-publishers.mjs              check every package
//   node scripts/store-verify-publishers.mjs --offline    only read the catalog and list the packages
//   node scripts/store-verify-publishers.mjs --root <dir> use another tree (the tests do this)
//
// <root> is the folder that holds catalog/ (default: packages/app/src-tauri, as in gen-store-catalog.mjs).
// Exit codes: 0 every package matches, 1 a mismatch, a missing publisher, a lookup error or a
// broken catalog file, 2 a usage error.

import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const DEFAULT_ROOT = join(repoRoot, 'packages', 'app', 'src-tauri');
const NPM_TIMEOUT_MS = 60_000;
const PYPI_TIMEOUT_MS = 30_000;

// Same rules as src/tui/install.rs (is_npm_package, is_pypi_package): no shell text can get in.
const NPM_NAME = /^(?:@[a-z0-9][a-z0-9._-]*\/)?[a-z0-9][a-z0-9._-]*$/;
const PYPI_NAME = /^[A-Za-z0-9_][A-Za-z0-9._-]{0,99}$/;

const USAGE = `usage: node scripts/store-verify-publishers.mjs [--offline] [--root <dir>]
  --offline     read the catalog and list the packages; do not ask the registries
  --root <dir>  folder that holds catalog/ (default: packages/app/src-tauri)`;

// ---------------------------------------------------------------------------
// A small TOML reader for the subset the catalog uses: tables, arrays of tables, dotted and quoted
// keys, single-line basic and literal strings, numbers, booleans, dates (kept as text), arrays and
// inline tables. Multi-line strings are refused with a message. Node has no TOML reader built in,
// and this script installs nothing, so the reader lives here.
// ---------------------------------------------------------------------------

const ESCAPES = { b: '\b', t: '\t', n: '\n', f: '\f', r: '\r', '"': '"', '\\': '\\' };

const hasOwn = (obj, key) => Object.prototype.hasOwnProperty.call(obj, key);
const isObject = (value) => typeof value === 'object' && value !== null && !Array.isArray(value);
// defineProperty, so a key such as "__proto__" stays data and never changes the prototype.
const setKey = (obj, key, value) =>
  Object.defineProperty(obj, key, { value, enumerable: true, writable: true, configurable: true });

export function parseToml(text) {
  const p = { s: String(text).replace(/^﻿/, ''), i: 0, line: 1 };
  const root = {};
  const defined = new Set();
  let table = root;
  for (;;) {
    skipBlank(p);
    if (p.i >= p.s.length) return root;
    if (p.s[p.i] === '[') {
      const isArray = p.s[p.i + 1] === '[';
      p.i += isArray ? 2 : 1;
      skipSpaces(p);
      const keys = parseKeyPath(p);
      skipSpaces(p);
      expect(p, ']');
      if (isArray) expect(p, ']');
      endOfLine(p);
      if (isArray) {
        table = openArrayTable(root, keys, p);
      } else {
        const name = keys.join('.');
        if (defined.has(name)) fail(p, `table [${name}] is defined twice`);
        defined.add(name);
        table = walk(root, keys, p);
      }
    } else {
      const keys = parseKeyPath(p);
      skipSpaces(p);
      expect(p, '=');
      skipSpaces(p);
      assign(table, keys, parseValue(p), p);
      endOfLine(p);
    }
  }
}

function fail(p, message) {
  throw new Error(`line ${p.line}: ${message}`);
}

function expect(p, ch) {
  if (p.s[p.i] !== ch) fail(p, `expected "${ch}"`);
  p.i += 1;
}

function skipSpaces(p) {
  while (p.s[p.i] === ' ' || p.s[p.i] === '\t') p.i += 1;
}

// Spaces, line ends and comments: the gaps between statements and inside arrays.
function skipBlank(p) {
  for (;;) {
    const c = p.s[p.i];
    if (c === ' ' || c === '\t' || c === '\r') p.i += 1;
    else if (c === '\n') {
      p.i += 1;
      p.line += 1;
    } else if (c === '#') {
      while (p.i < p.s.length && p.s[p.i] !== '\n') p.i += 1;
    } else return;
  }
}

// After a statement: only spaces and an optional comment may follow on the same line.
function endOfLine(p) {
  skipSpaces(p);
  if (p.s[p.i] === '#') while (p.i < p.s.length && p.s[p.i] !== '\n') p.i += 1;
  if (p.i >= p.s.length) return;
  if (p.s[p.i] === '\r') p.i += 1;
  if (p.s[p.i] !== '\n') fail(p, 'expected the end of the line');
}

function parseKeyPath(p) {
  const keys = [parseKey(p)];
  for (;;) {
    skipSpaces(p);
    if (p.s[p.i] !== '.') return keys;
    p.i += 1;
    skipSpaces(p);
    keys.push(parseKey(p));
  }
}

function parseKey(p) {
  const c = p.s[p.i];
  if (c === '"') return parseBasic(p);
  if (c === "'") return parseLiteral(p);
  const re = /[A-Za-z0-9_-]+/y;
  re.lastIndex = p.i;
  const m = re.exec(p.s);
  if (!m) fail(p, 'expected a key');
  p.i += m[0].length;
  return m[0];
}

function parseValue(p) {
  const c = p.s[p.i];
  if (p.s.startsWith('"""', p.i) || p.s.startsWith("'''", p.i)) {
    fail(p, 'multi-line strings are not supported in catalog files');
  }
  if (c === '"') return parseBasic(p);
  if (c === "'") return parseLiteral(p);
  if (c === '[') return parseArray(p);
  if (c === '{') return parseInline(p);
  const re = /[A-Za-z0-9_.:+-]+/y;
  re.lastIndex = p.i;
  const m = re.exec(p.s);
  if (!m) fail(p, 'expected a value');
  const token = m[0];
  p.i += token.length;
  if (token === 'true') return true;
  if (token === 'false') return false;
  if (/^[+-]?\d[\d_]*(\.\d[\d_]*)?([eE][+-]?\d+)?$/.test(token)) return Number(token.replace(/_/g, ''));
  if (/^\d{4}-\d{2}-\d{2}/.test(token)) return token;
  return fail(p, `unsupported value "${token}"`);
}

function parseBasic(p) {
  p.i += 1;
  let out = '';
  for (;;) {
    const c = p.s[p.i];
    if (c === undefined || c === '\n' || c === '\r') fail(p, 'unterminated string');
    if (c === '"') {
      p.i += 1;
      return out;
    }
    if (c === '\\') {
      const e = p.s[p.i + 1];
      if (e === 'u' || e === 'U') {
        const len = e === 'u' ? 4 : 8;
        const hex = p.s.slice(p.i + 2, p.i + 2 + len);
        if (hex.length !== len || !/^[0-9A-Fa-f]+$/.test(hex)) fail(p, 'bad unicode escape');
        out += String.fromCodePoint(parseInt(hex, 16));
        p.i += 2 + len;
      } else if (hasOwn(ESCAPES, e)) {
        out += ESCAPES[e];
        p.i += 2;
      } else fail(p, 'bad escape in string');
      continue;
    }
    out += c;
    p.i += 1;
  }
}

function parseLiteral(p) {
  const end = p.s.indexOf("'", p.i + 1);
  const eol = p.s.indexOf('\n', p.i + 1);
  if (end < 0 || (eol >= 0 && eol < end)) fail(p, 'unterminated string');
  const out = p.s.slice(p.i + 1, end);
  p.i = end + 1;
  return out;
}

function parseArray(p) {
  p.i += 1;
  const items = [];
  for (;;) {
    skipBlank(p);
    if (p.s[p.i] === ']') {
      p.i += 1;
      return items;
    }
    items.push(parseValue(p));
    skipBlank(p);
    if (p.s[p.i] === ',') {
      p.i += 1;
      continue;
    }
    expect(p, ']');
    return items;
  }
}

function parseInline(p) {
  p.i += 1;
  const obj = {};
  skipSpaces(p);
  if (p.s[p.i] === '}') {
    p.i += 1;
    return obj;
  }
  for (;;) {
    const keys = parseKeyPath(p);
    skipSpaces(p);
    expect(p, '=');
    skipSpaces(p);
    assign(obj, keys, parseValue(p), p);
    skipSpaces(p);
    if (p.s[p.i] === ',') {
      p.i += 1;
      skipSpaces(p);
      continue;
    }
    expect(p, '}');
    return obj;
  }
}

// Walks down `keys` from `root`, creating tables as needed. An array of tables means its last element.
function walk(root, keys, p) {
  let obj = root;
  for (const key of keys) {
    if (!hasOwn(obj, key)) setKey(obj, key, {});
    let next = obj[key];
    if (Array.isArray(next)) next = next[next.length - 1];
    if (!isObject(next)) fail(p, `"${key}" is not a table`);
    obj = next;
  }
  return obj;
}

function assign(table, keys, value, p) {
  const parent = walk(table, keys.slice(0, -1), p);
  const last = keys[keys.length - 1];
  if (hasOwn(parent, last)) fail(p, `"${last}" is defined twice`);
  setKey(parent, last, value);
}

function openArrayTable(root, keys, p) {
  const parent = walk(root, keys.slice(0, -1), p);
  const last = keys[keys.length - 1];
  if (!hasOwn(parent, last)) setKey(parent, last, []);
  if (!Array.isArray(parent[last])) fail(p, `"${last}" is not an array of tables`);
  const row = {};
  parent[last].push(row);
  return row;
}

// ---------------------------------------------------------------------------
// Repository addresses. Registries and entries write the same repository in different forms
// (git+https, .git suffix, ssh, scp-style). Compared as host + path, case-insensitive (GitHub
// owner and repo names are not case-sensitive).
// ---------------------------------------------------------------------------

export function normalizeRepo(value) {
  let text = String(value ?? '').trim();
  if (!text) return '';
  text = text.replace(/^git\+/i, '');
  // scp-style git address: git@github.com:owner/repo.git
  const scp = /^[^/\s@:]+@([^/\s:]+):(?!\/\/)(.+)$/.exec(text);
  if (scp) text = `https://${scp[1]}/${scp[2]}`;
  else if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(text)) text = `https://${text}`;
  let url;
  try {
    url = new URL(text);
  } catch {
    return text.toLowerCase();
  }
  const host = url.hostname.toLowerCase().replace(/^www\./, '');
  const path = url.pathname.toLowerCase().replace(/\/+$/, '').replace(/\.git$/, '');
  return `${host}${path}`;
}

// ---------------------------------------------------------------------------
// Reading the catalog: one entry per agent folder that names an npm or uv package.
// ---------------------------------------------------------------------------

function readToml(file, rel, problems) {
  try {
    return parseToml(readFileSync(file, 'utf8'));
  } catch (err) {
    problems.push(`${rel}: ${err.message}`);
    return null;
  }
}

export function loadCatalog(root) {
  const agentsDir = join(root, 'catalog', 'agents');
  const problems = [];
  const packages = [];
  if (!existsSync(agentsDir)) {
    problems.push(`no catalog/agents folder under ${root}`);
    return { packages, problems };
  }
  for (const id of readdirSync(agentsDir).sort()) {
    const dir = join(agentsDir, id);
    if (!statSync(dir).isDirectory()) continue;
    const rel = `catalog/agents/${id}`;
    const defFile = join(dir, 'definition.toml');
    if (!existsSync(defFile)) continue;
    const definition = readToml(defFile, `${rel}/definition.toml`, problems);
    const windows = definition?.install?.windows;
    if (!isObject(windows)) continue;
    const wanted = [
      ['npm', windows.npm],
      ['uv', windows.uv],
    ].filter(([, name]) => name !== undefined);
    if (wanted.length === 0) continue;

    let publisher = {};
    const entryFile = join(dir, 'entry.toml');
    if (!existsSync(entryFile)) problems.push(`${rel}/entry.toml is missing`);
    else {
      const entry = readToml(entryFile, `${rel}/entry.toml`, problems);
      if (entry && isObject(entry.publisher)) publisher = entry.publisher;
    }
    for (const [manager, name] of wanted) {
      if (typeof name !== 'string') {
        problems.push(`${rel}/definition.toml: install.windows.${manager} must be text`);
        continue;
      }
      const raw = publisher[manager === 'npm' ? 'npm_repo' : 'pypi_repo'];
      packages.push({ agent: id, manager, name, expected: typeof raw === 'string' ? raw.trim() : '' });
    }
  }
  return { packages, problems };
}

// ---------------------------------------------------------------------------
// The registries. Both are injected, so the tests never touch the network.
// ---------------------------------------------------------------------------

// `npm view <pkg> repository.url`. On Windows npm is npm.cmd, which needs a shell. The name was
// checked against NPM_NAME first, so nothing reaches the shell except a plain package name.
export function npmViewRepo(name) {
  const win = process.platform === 'win32';
  const r = spawnSync(win ? 'npm.cmd' : 'npm', ['view', name, 'repository.url'], {
    encoding: 'utf8',
    shell: win,
    timeout: NPM_TIMEOUT_MS,
    windowsHide: true,
  });
  if (r.error) throw new Error(`npm could not run: ${r.error.message}`);
  if (r.status !== 0) throw new Error(firstLine(r.stderr) || `npm view exited with ${r.status}`);
  return firstLine(r.stdout);
}

export async function fetchPypiJson(name) {
  const res = await fetch(`https://pypi.org/pypi/${encodeURIComponent(name)}/json`, {
    headers: { accept: 'application/json' },
    signal: AbortSignal.timeout(PYPI_TIMEOUT_MS),
  });
  if (!res.ok) throw new Error(`PyPI answered HTTP ${res.status}`);
  return res.json();
}

const DEFAULT_DEPS = { npmView: npmViewRepo, fetchPypi: fetchPypiJson };

function firstLine(text) {
  return String(text ?? '')
    .split('\n')
    .map((line) => line.trim())
    .find(Boolean) ?? '';
}

// The PyPI project_urls values, with the key each one came from.
function pypiLinks(json) {
  const urls = json?.info?.project_urls;
  if (!isObject(urls)) return [];
  return Object.entries(urls)
    .filter(([, value]) => typeof value === 'string' && value.trim())
    .map(([key, value]) => ({ key, value: value.trim() }));
}

const REGISTRY = { npm: 'npm', uv: 'PyPI' };

// What the registry says, and the link that matches the entry's publisher (null when none does).
async function lookup(pkg, deps) {
  const want = normalizeRepo(pkg.expected);
  if (pkg.manager === 'npm') {
    const found = String(((await deps.npmView(pkg.name)) ?? '')).trim();
    const matched = found && want && normalizeRepo(found) === want ? { key: 'repository.url', value: found } : null;
    return { found, matched };
  }
  const links = pypiLinks(await deps.fetchPypi(pkg.name));
  const matched = links.find((link) => want && normalizeRepo(link.value) === want) ?? null;
  const preferred = links.find((link) => /source|repo|code|git/i.test(link.key)) ?? links[0] ?? null;
  return { found: (matched ?? preferred)?.value ?? '', matched };
}

// One row of the report. Status: OK, MISMATCH, NO REPO (the registry names no repository),
// NO PUBLISHER (the entry has no publisher repository), ERROR (bad name or lookup failed),
// SKIPPED (--offline). Only OK and SKIPPED are not a problem.
async function checkOne(pkg, deps, offline) {
  const row = {
    agent: pkg.agent,
    registry: REGISTRY[pkg.manager],
    name: pkg.name,
    expected: pkg.expected,
    found: '',
    status: '',
    detail: '',
  };
  const valid = pkg.manager === 'npm' ? NPM_NAME.test(pkg.name) && pkg.name.length <= 214 : PYPI_NAME.test(pkg.name);
  if (!valid) return finish({ ...row, status: 'ERROR', detail: 'not a valid package name' });
  if (offline) return finish({ ...row, status: 'SKIPPED', detail: 'offline' });
  let result;
  try {
    result = await lookup(pkg, deps);
  } catch (err) {
    return finish({ ...row, status: 'ERROR', detail: firstLine(err.message) });
  }
  const { found, matched } = result;
  row.found = found;
  if (matched && pkg.manager === 'uv') row.detail = `project_urls: ${matched.key}`;
  let status;
  if (!pkg.expected) status = 'NO PUBLISHER';
  else if (matched) status = 'OK';
  else if (!found) status = 'NO REPO';
  else status = 'MISMATCH';
  return finish({ ...row, status });
}

function finish(row) {
  return { ...row, ok: row.status === 'OK' || row.status === 'SKIPPED' };
}

export async function checkPackages(packages, deps, { offline = false } = {}) {
  const rows = [];
  for (const pkg of packages) rows.push(await checkOne(pkg, deps, offline));
  return rows;
}

// A plain-text table. Columns are padded to the widest cell.
export function renderTable(rows) {
  const headers = ['Agent', 'Registry', 'Package', 'Publisher in entry', 'Registry says', 'Result'];
  const data = rows.map((r) => [
    r.agent,
    r.registry,
    r.name,
    r.expected || '(none)',
    r.found || '(none)',
    r.detail ? `${r.status} (${r.detail})` : r.status,
  ]);
  const widths = headers.map((h, i) => Math.max(h.length, ...data.map((cells) => cells[i].length)));
  const line = (cells) => cells.map((cell, i) => cell.padEnd(widths[i])).join('  ').trimEnd();
  return [line(headers), line(widths.map((w) => '-'.repeat(w))), ...data.map(line)].join('\n');
}

function parseArgs(argv) {
  const opts = { root: DEFAULT_ROOT, offline: false, help: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--offline') opts.offline = true;
    else if (arg === '--help' || arg === '-h') opts.help = true;
    else if (arg === '--root') {
      i += 1;
      if (!argv[i]) throw new Error('--root needs a folder');
      opts.root = argv[i];
    } else throw new Error(`unknown option: ${arg}`);
  }
  return opts;
}

export async function main(argv, deps = {}, io = console) {
  let opts;
  try {
    opts = parseArgs(argv);
  } catch (err) {
    io.error(`${err.message}\n${USAGE}`);
    return 2;
  }
  if (opts.help) {
    io.log(USAGE);
    return 0;
  }
  try {
    const root = resolve(opts.root);
    const { packages, problems } = loadCatalog(root);
    if (problems.length > 0) {
      for (const problem of problems) io.error(`catalog problem: ${problem}`);
      return 1;
    }
    if (packages.length === 0) {
      io.log(`No npm or uv packages in ${join(root, 'catalog', 'agents')}. Nothing to check.`);
      return 0;
    }
    const rows = await checkPackages(packages, { ...DEFAULT_DEPS, ...deps }, { offline: opts.offline });
    io.log(renderTable(rows));
    const failed = rows.filter((r) => !r.ok).length;
    if (opts.offline) {
      io.log(`Listed ${rows.length} package(s). Offline: the registries were not asked.`);
    } else {
      io.log(`Checked ${rows.length} package(s): ${rows.length - failed} match, ${failed} do not.`);
    }
    if (failed > 0) {
      io.error('Some packages do not come from their entry\'s publisher. Fix publisher.npm_repo or publisher.pypi_repo, or the package name, then run again.');
      return 1;
    }
    return 0;
  } catch (err) {
    io.error(err.message);
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).then((code) => {
    process.exitCode = code;
  });
}
