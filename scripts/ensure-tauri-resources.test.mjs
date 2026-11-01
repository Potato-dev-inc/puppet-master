// Tests for the freshness rule of scripts/ensure-tauri-resources.mjs and scripts/resource-stamp.mjs. Run with:
//   node --test scripts/ensure-tauri-resources.test.mjs
//
// The script works on the repo it sits in, so each test copies it (and the stamp helper) into a throwaway repo whose
// bundle scripts are small stand-ins that only count how often they ran. What is checked is the rule: a generated
// folder is built again when its sources changed, left alone when they did not, and a stub never sticks.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';
import { hashInputs, readStamp, STAMP_NAME, writeStamp } from './resource-stamp.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const made = [];

after(() => {
  for (const dir of made) rmSync(dir, { recursive: true, force: true });
});

function write(file, text) {
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, text, 'utf8');
}

/** A throwaway repo: the real ensure script and stamp helper, stand-in bundle scripts, tiny sources. */
function fakeRepo() {
  const root = mkdtempSync(join(tmpdir(), 'pm-ensure-'));
  made.push(root);
  for (const name of ['ensure-tauri-resources.mjs', 'resource-stamp.mjs']) {
    mkdirSync(join(root, 'scripts'), { recursive: true });
    copyFileSync(join(here, name), join(root, 'scripts', name));
  }
  write(join(root, 'package.json'), '{"type":"module"}\n');
  write(join(root, 'packages/potato-brain/src/pm.ts'), 'export const a = 1;\n');
  write(join(root, 'packages/potato-brain/persona/protocol.md'), 'protocol\n');
  write(join(root, 'packages/potato-brain/package.json'), '{"name":"b"}\n');
  write(join(root, 'packages/potato-sandbox/guest/run.sh'), 'echo\n');
  write(join(root, 'packages/potato-sandbox/recipes/base.json'), '{}\n');
  write(join(root, 'packages/potato-sandbox/viewer/embed.html'), '<p>\n');
  write(join(root, 'packages/potato-sandbox/viewer/novnc/core.js'), '//\n');
  write(join(root, 'packages/potato-sandbox/LICENSE-APACHE-2.0'), 'license\n');
  // Stand-ins that count their runs and stamp the way the real ones do.
  const stamp = "import { brainInputsHash, sandboxInputsHash, writeStamp } from './resource-stamp.mjs';";
  write(
    join(root, 'scripts/bundle-potato-brain.mjs'),
    `${stamp}
import { appendFileSync, cpSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'packages/app/src-tauri/resources/potato-brain');
mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'brain.mjs'), 'real bundle\\n');
cpSync(join(root, 'packages/potato-brain/persona'), join(out, 'persona'), { recursive: true });
writeStamp(out, brainInputsHash(root));
appendFileSync(join(root, 'brain-runs.txt'), 'x');
`,
  );
  write(
    join(root, 'scripts/bundle-potato-sandbox.mjs'),
    `${stamp}
import { appendFileSync, cpSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'packages/app/src-tauri/resources/potato-sandbox');
mkdirSync(out, { recursive: true });
for (const part of ['guest', 'recipes', 'viewer']) cpSync(join(root, 'packages/potato-sandbox', part), join(out, part), { recursive: true });
writeStamp(out, sandboxInputsHash(root));
appendFileSync(join(root, 'sandbox-runs.txt'), 'x');
`,
  );
  return root;
}

function ensure(root) {
  const run = spawnSync(process.execPath, [join(root, 'scripts', 'ensure-tauri-resources.mjs')], { cwd: root, encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
}

function runs(root, name) {
  const file = join(root, name);
  return existsSync(file) ? readFileSync(file, 'utf8').length : 0;
}

describe('hashInputs', () => {
  it('changes with a file, a new file, a removed file and an extra, and ignores the stamp and node_modules', () => {
    const root = fakeRepo();
    const paths = ['packages/potato-brain/src', 'packages/potato-brain/persona'];
    const base = hashInputs(root, paths);
    assert.equal(hashInputs(root, paths), base, 'same sources, same hash');
    write(join(root, 'packages/potato-brain/src/node_modules/x/index.js'), 'ignored');
    write(join(root, 'packages/potato-brain/src', STAMP_NAME), 'ignored');
    assert.equal(hashInputs(root, paths), base, 'node_modules and the stamp do not count');
    assert.notEqual(hashInputs(root, paths, ['esbuild 0.1']), base, 'a tool version counts');
    write(join(root, 'packages/potato-brain/src/pm.ts'), 'export const a = 2;\n');
    const edited = hashInputs(root, paths);
    assert.notEqual(edited, base);
    write(join(root, 'packages/potato-brain/persona/packs/fish/core.md'), 'new pack file');
    const added = hashInputs(root, paths);
    assert.notEqual(added, edited);
    rmSync(join(root, 'packages/potato-brain/persona/packs/fish/core.md'));
    assert.equal(hashInputs(root, paths), edited);
  });

  it('writes and reads a stamp, and an absent stamp reads as empty', () => {
    const root = fakeRepo();
    assert.equal(readStamp(join(root, 'nowhere')), '');
    writeStamp(root, 'abc');
    assert.equal(readStamp(root), 'abc');
  });
});

describe('ensure-tauri-resources: the brain and the sandbox files', () => {
  it('builds both on a first run and leaves them alone on the second', () => {
    const root = fakeRepo();
    ensure(root);
    assert.equal(runs(root, 'brain-runs.txt'), 1);
    assert.equal(runs(root, 'sandbox-runs.txt'), 1);
    ensure(root);
    assert.equal(runs(root, 'brain-runs.txt'), 1, 'nothing changed: no second build');
    assert.equal(runs(root, 'sandbox-runs.txt'), 1);
  });

  it('builds the brain again when its source changed, and only the brain', () => {
    const root = fakeRepo();
    ensure(root);
    write(join(root, 'packages/potato-brain/src/pm.ts'), 'export const a = 2; // a later change\n');
    ensure(root);
    assert.equal(runs(root, 'brain-runs.txt'), 2, 'a changed source is bundled again');
    assert.equal(runs(root, 'sandbox-runs.txt'), 1);
    ensure(root);
    assert.equal(runs(root, 'brain-runs.txt'), 2, 'and then it is fresh again');
  });

  it('builds the brain again when a persona file changed', () => {
    const root = fakeRepo();
    ensure(root);
    write(join(root, 'packages/potato-brain/persona/protocol.md'), 'a new rule\n');
    ensure(root);
    assert.equal(runs(root, 'brain-runs.txt'), 2);
  });

  it('stages the sandbox files again when their source changed', () => {
    const root = fakeRepo();
    ensure(root);
    write(join(root, 'packages/potato-sandbox/guest/run.sh'), 'echo changed\n');
    ensure(root);
    assert.equal(runs(root, 'sandbox-runs.txt'), 2);
    assert.equal(runs(root, 'brain-runs.txt'), 1);
  });

  it('replaces a bundle that has no stamp (an old one, or the stub of a failed build)', () => {
    const root = fakeRepo();
    ensure(root);
    const brain = join(root, 'packages/app/src-tauri/resources/potato-brain');
    writeFileSync(join(brain, 'brain.mjs'), 'process.exit(2);\n');
    rmSync(join(brain, STAMP_NAME));
    ensure(root);
    assert.equal(runs(root, 'brain-runs.txt'), 2);
    assert.equal(readFileSync(join(brain, 'brain.mjs'), 'utf8'), 'real bundle\n');
  });
});
