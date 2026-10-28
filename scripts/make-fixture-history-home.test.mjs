// Tests for scripts/make-fixture-history-home.mjs. Run: node --test scripts/make-fixture-history-home.test.mjs
// They write only into a temp folder of their own, never into the real home: the real-home refusals are
// checked by asking the builder, and it refuses before it writes anything.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { after, before, describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';
import { buildFixtureHome, parseArgs } from './make-fixture-history-home.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..');
const script = join(here, 'make-fixture-history-home.mjs');
const opencodeSource = join(repoRoot, 'packages/app/src-tauri/src/tui/opencode/fixtures/history/opencode.db');
const claudeChat = join('.claude', 'projects', 'C--Users-you-proj', '3f2a9c10-7b4e-4d21-9a6c-1e5f0b7d8a01.jsonl');
const codexNotesPlain = join('.codex', 'sessions', '2026', '08', '15', 'rollout-2026-08-15T09-30-00-0198b100-0000-7000-8000-0000000000b2.jsonl');
const codexNotesZst = `${codexNotesPlain}.zst`;
const codexArchive = join('.codex', 'sessions', '2026', '08', '16', 'rollout-2026-08-16T08-00-00-0198b300-0000-7000-8000-0000000000c3.jsonl.zst');
const EXTRA_CODEX_NAME = 'rollout-2026-09-01T10-00-00-0198c000-0000-7000-8000-0000000000d4.jsonl';

/** True when `path` is the real home or inside it. */
function insideHome(path) {
  const rel = relative(homedir(), resolve(path));
  return rel === '' || (rel !== '..' && !rel.startsWith('..') && !isAbsolute(rel));
}

let scratch;
let realHome;
let extras;

before(() => {
  scratch = mkdtempSync(join(tmpdir(), 'pm-bc22-fixture-test-'));
  realHome = join(scratch, 'real-home');
  mkdirSync(realHome);
  extras = join(scratch, 'extras');
  mkdirSync(extras);
});

after(() => {
  rmSync(scratch, { recursive: true, force: true });
});

describe('parseArgs', () => {
  it('reads the out-dir and the files that follow each extra flag', () => {
    const args = parseArgs(['out', '--extra-claude', 'a.jsonl', 'b.jsonl', '--extra-codex', 'rollout-x.jsonl']);
    assert.deepEqual(args, { outDir: 'out', extraClaude: ['a.jsonl', 'b.jsonl'], extraCodex: ['rollout-x.jsonl'] });
  });

  it('needs an out-dir first, rejects unknown options and stray arguments', () => {
    assert.throws(() => parseArgs([]), /missing <out-dir>/);
    assert.throws(() => parseArgs(['out', '--nope']), /unknown option --nope/);
    assert.throws(() => parseArgs(['out', 'stray']), /unexpected argument stray/);
  });
});

describe('buildFixtureHome layout', () => {
  let out;
  let extraClaudeFile;
  let extraCodexFile;

  before(() => {
    extraClaudeFile = join(extras, 'real-claude.jsonl');
    extraCodexFile = join(extras, EXTRA_CODEX_NAME);
    writeFileSync(extraClaudeFile, '{"type":"user","cwd":"C:\\\\Users\\\\you\\\\demo"}\n');
    writeFileSync(extraCodexFile, '{"type":"session_meta"}\n');
    out = join(scratch, 'out-layout');
  });

  it('copies the Claude, Codex and OpenCode fixtures into the home layout', () => {
    const result = buildFixtureHome({ outDir: out, realHome, env: {} });
    assert.equal(result.outDir, resolve(out));
    assert.deepEqual(result.counts, { claude: 4, codex: 4, opencode: 1 });
    for (const rel of [
      claudeChat,
      join('.claude', 'projects', 'C--Users-you-proj', '3f2a9c10-7b4e-4d21-9a6c-1e5f0b7d8a01', 'subagents', 'agent-x.jsonl'),
      codexNotesPlain,
      codexNotesZst,
      codexArchive,
      join('.local', 'share', 'opencode', 'opencode.db'),
    ]) {
      assert.ok(existsSync(join(out, rel)), `missing ${rel}`);
    }
    assert.deepEqual(
      readFileSync(join(out, '.local', 'share', 'opencode', 'opencode.db')),
      readFileSync(opencodeSource),
      'the OpenCode db is a byte-for-byte copy',
    );
  });

  it('puts each extra file where its agent looks, and keeps its bytes', () => {
    const withExtras = join(scratch, 'out-extras');
    buildFixtureHome({
      outDir: withExtras,
      extraClaude: [extraClaudeFile],
      extraCodex: [extraCodexFile],
      realHome,
      env: {},
    });
    assert.equal(
      readFileSync(join(withExtras, '.claude', 'projects', 'extra', 'real-claude.jsonl'), 'utf8'),
      readFileSync(extraClaudeFile, 'utf8'),
    );
    const codexDay = join(withExtras, '.codex', 'sessions', '2026', '09', '01', EXTRA_CODEX_NAME);
    assert.equal(readFileSync(codexDay, 'utf8'), readFileSync(extraCodexFile, 'utf8'), 'day folder comes from the file name');
  });
});

describe('buildFixtureHome refusals', () => {
  it('refuses the real home folder and writes nothing (default real home)', () => {
    const out = join(homedir(), 'pm-bc22-refusal-check-should-not-exist');
    assert.throws(() => buildFixtureHome({ outDir: out, env: {} }), /real home folder/);
    assert.equal(existsSync(out), false);
  });

  it('refuses a folder inside the real home it is told about, and writes nothing', () => {
    const out = join(realHome, 'fake-home');
    assert.throws(() => buildFixtureHome({ outDir: out, realHome, env: {} }), /real home folder/);
    assert.equal(existsSync(out), false);
  });

  it('refuses a folder inside a real agent store variable or the app data folder', () => {
    const cases = [
      { env: { CLAUDE_CONFIG_DIR: join(scratch, 'cfg') }, out: join(scratch, 'cfg', 'home'), label: 'CLAUDE_CONFIG_DIR' },
      { env: { CODEX_HOME: join(scratch, 'codex') }, out: join(scratch, 'codex', 'home'), label: 'CODEX_HOME' },
      { env: { XDG_DATA_HOME: join(scratch, 'xdg') }, out: join(scratch, 'xdg', 'home'), label: 'XDG_DATA_HOME' },
      {
        env: { APPDATA: join(scratch, 'roaming') },
        out: join(scratch, 'roaming', 'com.puppetmaster.app', 'home'),
        label: "the app's data folder",
      },
    ];
    for (const { env, out, label } of cases) {
      assert.throws(() => buildFixtureHome({ outDir: out, realHome, env }), new RegExp(label.replace(/[.$]/g, '\\$&')));
      assert.equal(existsSync(out), false, `${label}: nothing written`);
    }
  });

  it('refuses a folder inside this repository', () => {
    const out = join(repoRoot, 'bc22-should-not-exist');
    assert.throws(() => buildFixtureHome({ outDir: out, realHome, env: {} }), /this repository/);
    assert.equal(existsSync(out), false);
  });

  it('refuses a link that leads into the real home', (t) => {
    const link = join(scratch, 'link-to-home');
    try {
      symlinkSync(realHome, link, 'junction');
    } catch (err) {
      t.skip(`cannot create a link here: ${err.code}`);
      return;
    }
    assert.throws(() => buildFixtureHome({ outDir: join(link, 'fake'), realHome, env: {} }), /real home folder/);
    assert.equal(existsSync(join(realHome, 'fake')), false);
  });

  it('refuses a non-empty out-dir and leaves its files alone', () => {
    const out = join(scratch, 'not-empty');
    mkdirSync(out);
    writeFileSync(join(out, 'keep.txt'), 'mine');
    assert.throws(() => buildFixtureHome({ outDir: out, realHome, env: {} }), /not empty/);
    assert.equal(readFileSync(join(out, 'keep.txt'), 'utf8'), 'mine');
    assert.deepEqual(readdirSync(out), ['keep.txt']);
  });

  it('accepts an existing empty out-dir', () => {
    const out = join(scratch, 'empty-already');
    mkdirSync(out);
    const result = buildFixtureHome({ outDir: out, realHome, env: {} });
    assert.equal(result.counts.opencode, 1);
  });

  it('refuses a wrong extra file, a missing one, and two files that would land on one name', () => {
    const out = join(scratch, 'bad-extras');
    const notes = join(extras, 'notes.txt');
    writeFileSync(notes, 'x');
    assert.throws(() => buildFixtureHome({ outDir: out, extraClaude: [notes], realHome, env: {} }), /\.jsonl session file/);
    const chat = join(extras, 'chat.jsonl');
    writeFileSync(chat, 'x');
    assert.throws(() => buildFixtureHome({ outDir: out, extraCodex: [chat], realHome, env: {} }), /Codex rollout/);
    assert.throws(
      () => buildFixtureHome({ outDir: out, extraClaude: [join(extras, 'missing.jsonl')], realHome, env: {} }),
      /not a file: missing\.jsonl/,
    );
    const sameName = join(extras, 'same', 'rollout-2026-08-15T09-30-00-0198b100-0000-7000-8000-0000000000b2.jsonl');
    mkdirSync(join(extras, 'same'));
    writeFileSync(sameName, 'x');
    assert.throws(() => buildFixtureHome({ outDir: out, extraCodex: [sameName], realHome, env: {} }), /land on/);
    assert.equal(existsSync(out), false, 'every check ran before the first write');
  });
});

describe('command line', () => {
  it('exits 2 with the usage line when no out-dir is given', () => {
    const run = spawnSync(process.execPath, [script], { encoding: 'utf8' });
    assert.equal(run.status, 2);
    assert.match(run.stderr, /usage: node scripts\/make-fixture-history-home\.mjs/);
  });

  it('exits 1 and writes nothing for the real home', () => {
    const out = join(homedir(), 'pm-bc22-cli-refusal-check-should-not-exist');
    const run = spawnSync(process.execPath, [script, out], { encoding: 'utf8' });
    assert.equal(run.status, 1);
    assert.match(run.stderr, /real home folder/);
    assert.equal(run.stdout, '');
    assert.equal(existsSync(out), false);
  });

  it('prints the folder as the last stdout line', (t) => {
    if (insideHome(scratch)) {
      // On a PC whose temp folder sits inside the real home, the builder refuses every temp out-dir (correct).
      t.skip('the temp folder is inside the real home on this PC; the refusal test above covers it');
      return;
    }
    const out = join(scratch, 'cli-out');
    const run = spawnSync(process.execPath, [script, out], { encoding: 'utf8' });
    assert.equal(run.status, 0, run.stderr);
    const lines = run.stdout.trim().split(/\r?\n/);
    assert.equal(lines.at(-1), resolve(out));
    assert.match(run.stderr, /Your real home was not touched/);
  });
});
