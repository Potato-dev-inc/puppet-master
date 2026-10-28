// Tests for scripts/gen-store-catalog.mjs. Run with:
//   node --test scripts/gen-store-catalog.test.mjs
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { after, describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';
import { embeddedPath, listCatalogFiles, main, renderEmbedded } from './gen-store-catalog.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const SCRIPT = join(here, 'gen-store-catalog.mjs');
const FIXTURE = join(here, 'fixtures', 'store-catalog');
const SAMPLE_FILES = [
  'agents/alpha/definition.toml',
  'agents/alpha/entry.toml',
  'agents/alpha/fixtures/tui_idle.expect.toml',
  'agents/alpha/fixtures/tui_idle.txt',
  'agents/beta/entry.toml',
];

const tmpRoots = [];

// A fresh copy of the sample tree in the OS temp folder, so the repo fixtures are never written.
function freshRoot() {
  const base = mkdtempSync(join(tmpdir(), 'store-gen-'));
  tmpRoots.push(base);
  const root = join(base, 'root');
  cpSync(FIXTURE, root, { recursive: true });
  return root;
}

function quiet(t) {
  t.mock.method(console, 'log', () => {});
  t.mock.method(console, 'error', () => {});
}

after(() => {
  for (const dir of tmpRoots) rmSync(dir, { recursive: true, force: true });
});

describe('listCatalogFiles', () => {
  it('lists only .toml, .txt and .jsonl files under catalog/agents, sorted', () => {
    assert.deepEqual(listCatalogFiles(freshRoot()), SAMPLE_FILES);
  });

  it('treats a missing catalog folder as an empty catalog', () => {
    const root = freshRoot();
    rmSync(join(root, 'catalog'), { recursive: true, force: true });
    assert.deepEqual(listCatalogFiles(root), []);
  });

  it('refuses a folder name that could break an include_str! path', () => {
    const root = freshRoot();
    mkdirSync(join(root, 'catalog', 'agents', 'bad name'));
    writeFileSync(join(root, 'catalog', 'agents', 'bad name', 'entry.toml'), 'x = 1\n');
    assert.throws(() => listCatalogFiles(root), /catalog name not allowed.*bad name/);
  });
});

describe('renderEmbedded', () => {
  it('writes an empty list for an empty catalog', () => {
    const text = renderEmbedded([]);
    assert.match(text, /pub static FILES: &\[\(&str, &str\)\] = &\[\];/);
    assert.doesNotMatch(text, /include_str!/);
  });

  it('writes one include_str! row per file, pointing up to ../../catalog', () => {
    const text = renderEmbedded(['agents/beta/entry.toml']);
    assert.match(
      text,
      /\("agents\/beta\/entry\.toml", include_str!\("\.\.\/\.\.\/catalog\/agents\/beta\/entry\.toml"\)\),/,
    );
  });
});

describe('main (write and --check)', () => {
  it('writes embedded.rs and every include_str! path points at a real catalog file', (t) => {
    quiet(t);
    const root = freshRoot();
    assert.equal(main(['--root', root]), 0);
    const text = readFileSync(embeddedPath(root), 'utf8');
    const includes = [...text.matchAll(/include_str!\("([^"]+)"\)/g)].map((m) => m[1]);
    assert.equal(includes.length, SAMPLE_FILES.length);
    for (const rel of includes) {
      const target = resolve(root, 'src', 'agent_store', rel);
      assert.ok(existsSync(target), `missing ${rel}`);
    }
    assert.equal(text, renderEmbedded(SAMPLE_FILES));
  });

  it('--check exits 1 while embedded.rs is missing, then 0 after a write', (t) => {
    quiet(t);
    const root = freshRoot();
    assert.equal(main(['--root', root, '--check']), 1);
    assert.equal(main(['--root', root]), 0);
    assert.equal(main(['--root', root, '--check']), 0);
  });

  it('--check exits 1 when a catalog file was added after the last write', (t) => {
    quiet(t);
    const root = freshRoot();
    assert.equal(main(['--root', root]), 0);
    mkdirSync(join(root, 'catalog', 'agents', 'gamma'));
    writeFileSync(join(root, 'catalog', 'agents', 'gamma', 'entry.toml'), 'x = 1\n');
    assert.equal(main(['--root', root, '--check']), 1);
  });

  it('--check accepts a CRLF checkout of embedded.rs', (t) => {
    quiet(t);
    const root = freshRoot();
    assert.equal(main(['--root', root]), 0);
    const target = embeddedPath(root);
    writeFileSync(target, readFileSync(target, 'utf8').replace(/\n/g, '\r\n'));
    assert.equal(main(['--root', root, '--check']), 0);
  });

  it('a bad folder name fails the run and writes nothing', (t) => {
    quiet(t);
    const root = freshRoot();
    mkdirSync(join(root, 'catalog', 'agents', 'bad name'));
    writeFileSync(join(root, 'catalog', 'agents', 'bad name', 'entry.toml'), 'x = 1\n');
    assert.equal(main(['--root', root]), 1);
    assert.equal(existsSync(embeddedPath(root)), false);
  });

  it('an unknown option exits 2', (t) => {
    quiet(t);
    assert.equal(main(['--bogus']), 2);
  });
});

describe('command line', () => {
  it('runs when started directly and reports --check through the exit code', () => {
    const root = freshRoot();
    const check = () => spawnSync(process.execPath, [SCRIPT, '--root', root, '--check'], { encoding: 'utf8' });
    assert.equal(check().status, 1);
    const write = spawnSync(process.execPath, [SCRIPT, '--root', root], { encoding: 'utf8' });
    assert.equal(write.status, 0, write.stderr);
    assert.equal(check().status, 0, check().stderr);
  });
});
