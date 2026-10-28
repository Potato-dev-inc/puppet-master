// Tests for scripts/store-verify-publishers.mjs. Run with:
//   node --test scripts/store-verify-publishers.test.mjs
// No test touches the network: npm and PyPI are injected as fakes. The one real run is `--offline`.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { after, describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  checkPackages,
  loadCatalog,
  main,
  normalizeRepo,
  parseToml,
  renderTable,
} from './store-verify-publishers.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const SCRIPT = join(here, 'store-verify-publishers.mjs');
const REAL_AGENTS = join(here, '..', 'packages', 'app', 'src-tauri', 'catalog', 'agents');

const tmpRoots = [];
after(() => {
  for (const dir of tmpRoots) rmSync(dir, { recursive: true, force: true });
});

// A catalog tree in the OS temp folder. `files` maps paths relative to the root to file text.
function makeTree(files = {}) {
  const base = mkdtempSync(join(tmpdir(), 'store-publishers-'));
  tmpRoots.push(base);
  for (const [rel, text] of Object.entries(files)) {
    const full = join(base, ...rel.split('/'));
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, text);
  }
  return base;
}

// The files of one agent: a definition with its install.windows table and an entry with [publisher].
function agent(id, { npm, uv, npmRepo = '', pypiRepo = '', entry = true } = {}) {
  const install = ['[install.windows]', `source = "https://example.com/${id}"`];
  if (npm !== undefined) install.push(`npm = "${npm}"`);
  if (uv !== undefined) install.push(`uv = "${uv}"`);
  const files = {
    [`catalog/agents/${id}/definition.toml`]: `schema_version = 1\n\n[agent]\nid = "${id}"\nlabel = "${id}"\n\n${install.join('\n')}\n`,
  };
  if (entry) {
    files[`catalog/agents/${id}/entry.toml`] =
      `schema_version = 1\n\n[store]\nid = "${id}"\n\n[publisher]\nnpm_repo = "${npmRepo}"\npypi_repo = "${pypiRepo}"\n`;
  }
  return files;
}

// Fake `npm view`: answers from a map. An Error value is thrown, like a failed npm call.
function fakeNpm(answers = {}) {
  const calls = [];
  const npmView = (name) => {
    calls.push(name);
    const answer = answers[name];
    if (answer instanceof Error) throw answer;
    return answer ?? '';
  };
  return { npmView, calls };
}

// Fake PyPI JSON API: answers from a map of project name to the JSON document (or an Error).
function fakePypi(answers = {}) {
  const calls = [];
  const fetchPypi = async (name) => {
    calls.push(name);
    const answer = answers[name];
    if (answer instanceof Error) throw answer;
    return answer ?? { info: {} };
  };
  return { fetchPypi, calls };
}

function memoryIo() {
  const out = [];
  const err = [];
  return {
    io: { log: (m) => out.push(String(m)), error: (m) => err.push(String(m)) },
    out: () => out.join('\n'),
    err: () => err.join('\n'),
  };
}

// Runs main() on a fresh tree and returns the exit code and the printed text.
async function run(files, { deps = {}, argv = [] } = {}) {
  const root = makeTree(files);
  const memory = memoryIo();
  const code = await main(['--root', root, ...argv], deps, memory.io);
  return { code, out: memory.out(), err: memory.err() };
}

describe('parseToml', () => {
  it('reads tables, arrays of tables, inline tables, quoted keys and arrays', () => {
    const doc = parseToml(`# a comment
schema_version = 1
[store]
id = "alpha" # trailing comment
tier = 'verified'
ambiguous_name = false
categories = ["general",
  "open_source",]
[[headless.optional]]
when = "model"
[[headless.optional]]
when = "resume"
[headless.output.session]
where = { "~data.sessionId" = true, type = "runFinished" }
pick = "first"
[publisher]
script_hosts = []
`);
    assert.equal(doc.schema_version, 1);
    assert.equal(doc.store.id, 'alpha');
    assert.equal(doc.store.tier, 'verified');
    assert.equal(doc.store.ambiguous_name, false);
    assert.deepEqual(doc.store.categories, ['general', 'open_source']);
    assert.deepEqual(doc.headless.optional, [{ when: 'model' }, { when: 'resume' }]);
    assert.deepEqual(doc.headless.output.session.where, { '~data.sessionId': true, type: 'runFinished' });
    assert.deepEqual(doc.publisher.script_hosts, []);
  });

  it('reads escapes, dates as text, numbers with underscores and floats', () => {
    const doc = parseToml('a = "x\\ty\\u00e9"\nd = 2026-10-28\nn = 1_000\nf = 1.5\n');
    assert.equal(doc.a, 'x\tyé');
    assert.equal(doc.d, '2026-10-28');
    assert.equal(doc.n, 1000);
    assert.equal(doc.f, 1.5);
  });

  it('names the line of a syntax error', () => {
    assert.throws(() => parseToml('a = 1\nb = \n'), /line 2: expected a value/);
    assert.throws(() => parseToml('a = "open\n'), /line 1: unterminated string/);
  });

  it('refuses a duplicate key and a duplicate table', () => {
    assert.throws(() => parseToml('a = 1\na = 2\n'), /"a" is defined twice/);
    assert.throws(() => parseToml('[x]\n[x]\n'), /table \[x\] is defined twice/);
  });

  it('refuses multi-line strings with a clear message', () => {
    assert.throws(() => parseToml('a = """one\ntwo"""\n'), /multi-line strings are not supported/);
  });

  it('keeps a key named __proto__ as data', () => {
    const doc = parseToml('"__proto__" = 1\n');
    assert.ok(Object.prototype.hasOwnProperty.call(doc, '__proto__'));
    assert.equal(Object.getPrototypeOf(doc), Object.prototype);
  });

  it('reads every shipped catalog file', () => {
    let count = 0;
    for (const id of readdirSync(REAL_AGENTS)) {
      const dir = join(REAL_AGENTS, id);
      if (!statSync(dir).isDirectory()) continue;
      for (const name of readdirSync(dir)) {
        if (!name.endsWith('.toml')) continue;
        count += 1;
        assert.doesNotThrow(() => parseToml(readFileSync(join(dir, name), 'utf8')), `${id}/${name}`);
      }
    }
    assert.ok(count > 0, 'the shipped catalog has .toml files to read');
  });
});

describe('normalizeRepo', () => {
  it('treats the same repository in its different forms as equal', () => {
    const want = normalizeRepo('https://github.com/QwenLM/qwen-code');
    for (const form of [
      'https://github.com/qwenlm/qwen-code',
      'git+https://github.com/QwenLM/qwen-code.git',
      'https://github.com/QwenLM/qwen-code/',
      'git@github.com:QwenLM/qwen-code.git',
      'ssh://git@github.com/QwenLM/qwen-code.git',
      'git://github.com/QwenLM/qwen-code',
      'https://www.github.com/QwenLM/qwen-code.git',
    ]) {
      assert.equal(normalizeRepo(form), want, form);
    }
  });

  it('keeps different repositories apart, and an empty value empty', () => {
    assert.notEqual(normalizeRepo('https://github.com/vendor-a/tool'), normalizeRepo('https://github.com/vendor-b/tool'));
    assert.equal(normalizeRepo('  '), '');
    assert.equal(normalizeRepo(undefined), '');
  });
});

describe('loadCatalog', () => {
  it('collects npm and uv packages with the publisher repository from the entry', () => {
    const root = makeTree({
      ...agent('alpha', {
        npm: 'test-alpha-cli',
        uv: 'test-alpha-py',
        npmRepo: 'https://github.com/vendor-a/alpha',
        pypiRepo: 'https://github.com/vendor-a/alpha-py',
      }),
      ...agent('beta', {}),
      ...agent('gamma', { npm: 'test-gamma-cli' }),
    });
    // beta has no install table, so it is not in the list.
    const catalog = loadCatalog(root);
    assert.deepEqual(catalog.problems, []);
    assert.deepEqual(catalog.packages, [
      { agent: 'alpha', manager: 'npm', name: 'test-alpha-cli', expected: 'https://github.com/vendor-a/alpha' },
      { agent: 'alpha', manager: 'uv', name: 'test-alpha-py', expected: 'https://github.com/vendor-a/alpha-py' },
      { agent: 'gamma', manager: 'npm', name: 'test-gamma-cli', expected: '' },
    ]);
  });

  it('reports a broken entry file and a missing entry.toml as problems', () => {
    const root = makeTree({
      ...agent('delta', { npm: 'test-delta-cli' }),
      'catalog/agents/delta/entry.toml': 'a = \n',
      ...agent('epsilon', { npm: 'test-epsilon-cli', entry: false }),
    });
    const { problems } = loadCatalog(root);
    assert.equal(problems.length, 2);
    assert.match(problems[0], /delta\/entry\.toml: line 1/);
    assert.match(problems[1], /epsilon\/entry\.toml is missing/);
  });

  it('reports a missing catalog folder as a problem', () => {
    const { packages, problems } = loadCatalog(makeTree({}));
    assert.deepEqual(packages, []);
    assert.match(problems[0], /no catalog\/agents folder/);
  });

  it('refuses a package name that is not text', () => {
    const root = makeTree({
      'catalog/agents/zeta/definition.toml': '[install.windows]\nsource = "https://example.com/z"\nnpm = 3\n',
      'catalog/agents/zeta/entry.toml': '[publisher]\nnpm_repo = "https://github.com/x/z"\n',
    });
    assert.match(loadCatalog(root).problems[0], /install\.windows\.npm must be text/);
  });
});

describe('npm packages (fake npm view)', () => {
  it('a package from the publisher repository is OK and the run passes', async () => {
    const npm = fakeNpm({ 'test-alpha-cli': 'git+https://github.com/Vendor-A/alpha.git' });
    const result = await run(agent('alpha', { npm: 'test-alpha-cli', npmRepo: 'https://github.com/vendor-a/alpha' }), {
      deps: { npmView: npm.npmView },
    });
    assert.equal(result.code, 0, result.out + result.err);
    assert.match(result.out, /alpha\s+npm\s+test-alpha-cli\s+https:\/\/github\.com\/vendor-a\/alpha\s+git\+https:\/\/github\.com\/Vendor-A\/alpha\.git\s+OK/);
    assert.match(result.out, /Checked 1 package\(s\): 1 match, 0 do not\./);
    assert.deepEqual(npm.calls, ['test-alpha-cli']);
  });

  it('a package from another repository is a MISMATCH and the run fails', async () => {
    const npm = fakeNpm({ 'test-alpha-cli': 'https://github.com/vendor-b/alpha' });
    const result = await run(agent('alpha', { npm: 'test-alpha-cli', npmRepo: 'https://github.com/vendor-a/alpha' }), {
      deps: { npmView: npm.npmView },
    });
    assert.equal(result.code, 1);
    assert.match(result.out, /MISMATCH/);
    assert.match(result.out, /vendor-a\/alpha/);
    assert.match(result.out, /vendor-b\/alpha/);
    assert.match(result.err, /do not come from their entry's publisher/);
  });

  it('a registry that names no repository is NO REPO and the run fails', async () => {
    const npm = fakeNpm({ 'test-alpha-cli': '' });
    const result = await run(agent('alpha', { npm: 'test-alpha-cli', npmRepo: 'https://github.com/vendor-a/alpha' }), {
      deps: { npmView: npm.npmView },
    });
    assert.equal(result.code, 1);
    assert.match(result.out, /NO REPO/);
  });

  it('a failed npm view is ERROR with its first line, and the run fails', async () => {
    const npm = fakeNpm({ 'test-alpha-cli': new Error('npm error code E404\nnpm error 404 Not Found') });
    const result = await run(agent('alpha', { npm: 'test-alpha-cli', npmRepo: 'https://github.com/vendor-a/alpha' }), {
      deps: { npmView: npm.npmView },
    });
    assert.equal(result.code, 1);
    assert.match(result.out, /ERROR \(npm error code E404\)/);
  });

  it('an entry without a publisher repository is NO PUBLISHER and the run fails', async () => {
    const npm = fakeNpm({ 'test-alpha-cli': 'https://github.com/vendor-a/alpha' });
    const result = await run(agent('alpha', { npm: 'test-alpha-cli' }), { deps: { npmView: npm.npmView } });
    assert.equal(result.code, 1);
    assert.match(result.out, /NO PUBLISHER/);
  });

  it('a package name with shell text is refused before any lookup', async () => {
    for (const bad of ['test-alpha-cli && calc', '-g', '@scope', 'Test-Alpha', 'a b']) {
      const npm = fakeNpm({});
      const result = await run(agent('alpha', { npm: bad, npmRepo: 'https://github.com/vendor-a/alpha' }), {
        deps: { npmView: npm.npmView },
      });
      assert.equal(result.code, 1, bad);
      assert.match(result.out, /ERROR \(not a valid package name\)/, bad);
      assert.deepEqual(npm.calls, [], bad);
    }
  });
});

describe('uv packages (fake PyPI JSON API)', () => {
  const pypiRepo = 'https://github.com/vendor-a/alpha-py';

  it('a project_urls value from the publisher repository is OK', async () => {
    const pypi = fakePypi({
      'test-alpha-py': { info: { project_urls: { Homepage: 'https://example.com', Source: pypiRepo } } },
    });
    const result = await run(agent('alpha', { uv: 'test-alpha-py', pypiRepo }), { deps: { fetchPypi: pypi.fetchPypi } });
    assert.equal(result.code, 0, result.out + result.err);
    assert.match(result.out, /PyPI\s+test-alpha-py.*OK \(project_urls: Source\)/);
    assert.deepEqual(pypi.calls, ['test-alpha-py']);
  });

  it('project_urls that name another repository is a MISMATCH', async () => {
    const pypi = fakePypi({
      'test-alpha-py': { info: { project_urls: { Source: 'https://github.com/vendor-b/alpha-py' } } },
    });
    const result = await run(agent('alpha', { uv: 'test-alpha-py', pypiRepo }), { deps: { fetchPypi: pypi.fetchPypi } });
    assert.equal(result.code, 1);
    assert.match(result.out, /MISMATCH/);
    assert.match(result.out, /vendor-b\/alpha-py/);
  });

  it('project_urls with no link is NO REPO', async () => {
    const pypi = fakePypi({ 'test-alpha-py': { info: {} } });
    const result = await run(agent('alpha', { uv: 'test-alpha-py', pypiRepo }), { deps: { fetchPypi: pypi.fetchPypi } });
    assert.equal(result.code, 1);
    assert.match(result.out, /NO REPO/);
  });

  it('a PyPI error is ERROR and the run fails', async () => {
    const pypi = fakePypi({ 'test-alpha-py': new Error('PyPI answered HTTP 404') });
    const result = await run(agent('alpha', { uv: 'test-alpha-py', pypiRepo }), { deps: { fetchPypi: pypi.fetchPypi } });
    assert.equal(result.code, 1);
    assert.match(result.out, /ERROR \(PyPI answered HTTP 404\)/);
  });

  it('a uv name with a leading dash is refused before any lookup', async () => {
    const pypi = fakePypi({});
    const result = await run(agent('alpha', { uv: '--help', pypiRepo }), { deps: { fetchPypi: pypi.fetchPypi } });
    assert.equal(result.code, 1);
    assert.match(result.out, /not a valid package name/);
    assert.deepEqual(pypi.calls, []);
  });
});

describe('checkPackages and the run', () => {
  it('--offline lists the packages and asks no registry', async () => {
    const npm = fakeNpm({});
    const pypi = fakePypi({});
    const result = await run(
      { ...agent('alpha', { npm: 'test-alpha-cli', npmRepo: 'https://github.com/vendor-a/alpha' }), ...agent('beta', { uv: 'test-beta-py', pypiRepo: 'https://github.com/vendor-a/beta' }) },
      { argv: ['--offline'], deps: { npmView: npm.npmView, fetchPypi: pypi.fetchPypi } },
    );
    assert.equal(result.code, 0, result.out + result.err);
    assert.deepEqual(npm.calls, []);
    assert.deepEqual(pypi.calls, []);
    assert.match(result.out, /SKIPPED/);
    assert.match(result.out, /Listed 2 package\(s\)\. Offline: the registries were not asked\./);
  });

  it('a catalog with no npm or uv package has nothing to check and passes', async () => {
    const result = await run({ 'catalog/agents/beta/definition.toml': '[agent]\nid = "beta"\n' });
    assert.equal(result.code, 0);
    assert.match(result.out, /Nothing to check\./);
  });

  it('a broken catalog file stops the run before any lookup', async () => {
    const npm = fakeNpm({});
    const result = await run(
      {
        ...agent('alpha', { npm: 'test-alpha-cli', npmRepo: 'https://github.com/vendor-a/alpha' }),
        'catalog/agents/broken/definition.toml': 'x = \n',
      },
      { deps: { npmView: npm.npmView } },
    );
    assert.equal(result.code, 1);
    assert.match(result.err, /catalog problem: catalog\/agents\/broken\/definition\.toml: line 1/);
    assert.deepEqual(npm.calls, []);
  });

  it('an unknown option is a usage error (exit 2)', async () => {
    const memory = memoryIo();
    assert.equal(await main(['--nope'], {}, memory.io), 2);
    assert.match(memory.err(), /unknown option: --nope/);
  });

  it('--help prints the usage and exits 0', async () => {
    const memory = memoryIo();
    assert.equal(await main(['--help'], {}, memory.io), 0);
    assert.match(memory.out(), /usage: node scripts\/store-verify-publishers\.mjs/);
  });

  it('checkPackages returns one row per package in catalog order', async () => {
    const npm = fakeNpm({ 'test-alpha-cli': 'https://github.com/vendor-a/alpha' });
    const rows = await checkPackages(
      [{ agent: 'alpha', manager: 'npm', name: 'test-alpha-cli', expected: 'https://github.com/vendor-a/alpha' }],
      { npmView: npm.npmView },
    );
    assert.equal(rows.length, 1);
    assert.equal(rows[0].status, 'OK');
    assert.equal(rows[0].ok, true);
  });

  it('renderTable shows (none) for an empty value and pads the columns', () => {
    const text = renderTable([
      { agent: 'a', registry: 'npm', name: 'x', expected: '', found: 'https://github.com/o/r', status: 'NO PUBLISHER', detail: '' },
      { agent: 'long-agent', registry: 'PyPI', name: 'y', expected: 'https://github.com/o/s', found: '', status: 'NO REPO', detail: '' },
    ]);
    const [header, rule, first, second] = text.split('\n');
    assert.match(header, /^Agent\s+Registry\s+Package\s+Publisher in entry\s+Registry says\s+Result$/);
    assert.match(rule, /^-+(\s+-+)*$/);
    assert.match(first, /\(none\)\s+https:\/\/github\.com\/o\/r\s+NO PUBLISHER$/);
    assert.match(second, /^long-agent\s+PyPI\s+y\s+https:\/\/github\.com\/o\/s\s+\(none\)\s+NO REPO$/);
  });
});

describe('the command line', () => {
  it('runs as a script with --offline on a tree and exits 0 without the network', () => {
    const root = makeTree(agent('alpha', { npm: 'test-alpha-cli', npmRepo: 'https://github.com/vendor-a/alpha' }));
    const r = spawnSync(process.execPath, [SCRIPT, '--offline', '--root', root], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stdout, /test-alpha-cli/);
  });

  it('exits 1 when the catalog folder is missing', () => {
    const root = resolve(makeTree({}), 'not-there');
    const r = spawnSync(process.execPath, [SCRIPT, '--offline', '--root', root], { encoding: 'utf8' });
    assert.equal(r.status, 1);
    assert.match(r.stderr, /no catalog\/agents folder/);
  });
});
