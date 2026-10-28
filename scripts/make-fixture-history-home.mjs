#!/usr/bin/env node
/**
 * Build a fake home folder for the "Bring All My Chats" live check (BC-22).
 *
 *   node scripts/make-fixture-history-home.mjs <out-dir> [--extra-claude <file>...] [--extra-codex <file>...]
 *
 * Copies the synthetic history fixtures of the BC-4, BC-15 and BC-16 tests into a fake home layout:
 *   <out-dir>/.claude/projects/...            from tui/claude/fixtures/history/projects
 *   <out-dir>/.codex/sessions/YYYY/MM/DD/...  from tui/codex/fixtures/history/sessions
 *   <out-dir>/.local/share/opencode/opencode.db  from tui/opencode/fixtures/history/opencode.db
 *
 * --extra-claude <file>  adds one real Claude session file (.jsonl) under .claude/projects/extra/
 * --extra-codex <file>   adds one real Codex rollout (rollout-*.jsonl or .jsonl.zst) under
 *                        .codex/sessions/YYYY/MM/DD/ (the day is read from the file name, else the file date)
 * Put <out-dir> first. Each --extra flag takes the files that follow it.
 *
 * Safety:
 * - Refuses an out-dir that is the real home folder or inside it, or inside CLAUDE_CONFIG_DIR, CODEX_HOME,
 *   XDG_DATA_HOME, the app's AppData folders, or this repository. Links are resolved first.
 * - Refuses an out-dir that exists and is not empty. Every check runs before anything is written.
 * - Never reads the real home: only the repo fixtures and the files you name.
 * - Errors print no chat text. The copied files are verbatim, so the folder is private: keep it out of git.
 *
 * Prints the folder (last stdout line), so a live check can pass it as PUPPET_MASTER_HISTORY_HOME.
 */
import { copyFileSync, existsSync, mkdirSync, readdirSync, realpathSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const TUI = join(repoRoot, 'packages/app/src-tauri/src/tui');
const CLAUDE_SRC = join(TUI, 'claude/fixtures/history/projects');
const CODEX_SRC = join(TUI, 'codex/fixtures/history/sessions');
const OPENCODE_DB = join(TUI, 'opencode/fixtures/history/opencode.db');
const APP_FOLDER = 'com.puppetmaster.app';
const USAGE = 'usage: node scripts/make-fixture-history-home.mjs <out-dir> [--extra-claude <file>...] [--extra-codex <file>...]';

/** Reads the command line. Throws a plain message for a bad one. */
export function parseArgs(argv) {
  const args = { outDir: null, extraClaude: [], extraCodex: [] };
  let list = null;
  for (const arg of argv) {
    if (arg === '--extra-claude') list = args.extraClaude;
    else if (arg === '--extra-codex') list = args.extraCodex;
    else if (arg.startsWith('--')) throw new Error(`unknown option ${arg}`);
    else if (list) list.push(arg);
    else if (args.outDir === null) args.outDir = arg;
    else throw new Error(`unexpected argument ${arg}; put <out-dir> first`);
  }
  if (!args.outDir) throw new Error('missing <out-dir>');
  return args;
}

/** The nearest existing ancestor of `path`, resolved through links, with the missing rest put back. */
function canonical(path) {
  let current = resolve(path);
  const rest = [];
  while (!existsSync(current) && dirname(current) !== current) {
    rest.unshift(basename(current));
    current = dirname(current);
  }
  return join(realpathSync(current), ...rest);
}

/** True when `child` is `parent` or inside it (case-insensitive on Windows). */
function isInside(child, parent) {
  const rel = relative(parent, child);
  return rel === '' || (rel !== '..' && !rel.startsWith(`..${sep}`) && !isAbsolute(rel));
}

/** Folders the fake home must never be written into. */
function guardFolders({ realHome, env }) {
  return [
    { label: 'the real home folder', path: realHome },
    { label: 'CLAUDE_CONFIG_DIR', path: env.CLAUDE_CONFIG_DIR },
    { label: 'CODEX_HOME', path: env.CODEX_HOME },
    { label: 'XDG_DATA_HOME', path: env.XDG_DATA_HOME },
    { label: "the app's data folder", path: env.APPDATA && join(env.APPDATA, APP_FOLDER) },
    { label: "the app's data folder", path: env.LOCALAPPDATA && join(env.LOCALAPPDATA, APP_FOLDER) },
    { label: 'this repository', path: repoRoot },
  ].filter((guard) => guard.path);
}

/** Every file under `dir`, as '/'-separated paths relative to it, sorted. */
function listFiles(dir) {
  const found = [];
  const walk = (folder, prefix) => {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) walk(join(folder, entry.name), rel);
      else if (entry.isFile()) found.push(rel);
    }
  };
  walk(dir, '');
  return found.sort();
}

const pad = (n) => String(n).padStart(2, '0');

/** Codex keeps a rollout under YYYY/MM/DD; the date is in the file name, else the file's local date. */
function codexDay(file) {
  const name = basename(file);
  const named = /^rollout-(\d{4})-(\d{2})-(\d{2})T/.exec(name);
  if (named) return [named[1], named[2], named[3]];
  const d = statSync(file).mtime;
  return [String(d.getFullYear()), pad(d.getMonth() + 1), pad(d.getDate())];
}

/** The copy list: fixtures first, then the extra files the person named. Throws on a bad input. */
function planCopies(outDir, extraClaude, extraCodex) {
  const plan = [];
  for (const rel of listFiles(CLAUDE_SRC)) {
    plan.push({ kind: 'claude', from: join(CLAUDE_SRC, ...rel.split('/')), to: join(outDir, '.claude', 'projects', ...rel.split('/')) });
  }
  for (const rel of listFiles(CODEX_SRC)) {
    plan.push({ kind: 'codex', from: join(CODEX_SRC, ...rel.split('/')), to: join(outDir, '.codex', 'sessions', ...rel.split('/')) });
  }
  plan.push({ kind: 'opencode', from: OPENCODE_DB, to: join(outDir, '.local', 'share', 'opencode', 'opencode.db') });
  for (const file of extraClaude) {
    const name = basename(file);
    if (!name.endsWith('.jsonl')) throw new Error('--extra-claude takes a .jsonl session file');
    plan.push({ kind: 'claude', from: resolve(file), to: join(outDir, '.claude', 'projects', 'extra', name) });
  }
  for (const file of extraCodex) {
    const name = basename(file);
    if (!/^rollout-.+\.jsonl(\.zst)?$/.test(name)) throw new Error('--extra-codex takes a Codex rollout (rollout-*.jsonl or .jsonl.zst)');
    plan.push({ kind: 'codex', from: resolve(file), to: join(outDir, '.codex', 'sessions', ...codexDay(file), name) });
  }
  for (const item of plan) {
    if (!statSync(item.from, { throwIfNoEntry: false })?.isFile()) throw new Error(`not a file: ${basename(item.from)}`);
  }
  const seen = new Set();
  for (const item of plan) {
    const key = item.to.toLowerCase();
    if (seen.has(key)) throw new Error(`two files would land on ${basename(item.to)}; rename one`);
    seen.add(key);
  }
  return plan;
}

/**
 * Builds the fake home. Throws (and writes nothing) when the out-dir is a refused place, is not empty,
 * or a named file is wrong. `realHome` and `env` are injectable for tests; the CLI uses the real ones.
 */
export function buildFixtureHome({ outDir, extraClaude = [], extraCodex = [], realHome = homedir(), env = process.env }) {
  const target = canonical(outDir);
  for (const guard of guardFolders({ realHome, env })) {
    if (isInside(target, canonical(guard.path))) {
      throw new Error(`refusing to write into ${guard.label}; pick a new folder, for example D:\\pm-night\\bc-fixture-home`);
    }
  }
  if (existsSync(target)) {
    if (!statSync(target).isDirectory()) throw new Error('refusing: the out-dir is not a folder');
    if (readdirSync(target).length > 0) throw new Error('refusing: the out-dir is not empty; pick a new folder');
  }
  const plan = planCopies(target, extraClaude, extraCodex);
  mkdirSync(target, { recursive: true });
  const counts = { claude: 0, codex: 0, opencode: 0 };
  for (const item of plan) {
    mkdirSync(dirname(item.to), { recursive: true });
    copyFileSync(item.from, item.to);
    counts[item.kind] += 1;
  }
  return { outDir: target, counts };
}

function main(argv) {
  let args;
  try {
    args = parseArgs(argv);
  } catch (err) {
    console.error(`make-fixture-history-home: ${err.message}\n${USAGE}`);
    return 2;
  }
  try {
    const { outDir, counts } = buildFixtureHome(args);
    console.error(`make-fixture-history-home: claude ${counts.claude} files, codex ${counts.codex} files, opencode ${counts.opencode} db. Your real home was not touched.`);
    console.log(outDir);
    return 0;
  } catch (err) {
    console.error(`make-fixture-history-home: ${err.message}`);
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2));
}
