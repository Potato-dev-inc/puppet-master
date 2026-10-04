/**
 * `puppet-master watch` flag validation. An unknown flag (for example `--timeout 5`, where the
 * documented flag is `--timeout-ms`) used to be ignored silently and the watch ran with the
 * 1 hour default.
 */
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { describe, it } from 'node:test';
import { WATCH_USAGE, validateWatchFlags } from './watch-args.js';

describe('validateWatchFlags', () => {
  it('accepts the documented flags', () => {
    assert.equal(
      validateWatchFlags({ 'project-path': 'C:/p', 'timeout-ms': '5000', stream: true }),
      null,
    );
    assert.equal(validateWatchFlags({ project: 'C:/p' }), null);
    assert.equal(validateWatchFlags({}), null);
  });

  it('rejects an unknown flag and names the documented one', () => {
    const problem = validateWatchFlags({ timeout: '5' });
    assert.ok(problem);
    assert.match(problem!, /--timeout\b/);
    assert.match(WATCH_USAGE, /--timeout-ms/);
  });

  it('rejects a non-numeric or non-positive timeout', () => {
    assert.match(validateWatchFlags({ 'timeout-ms': 'soon' }) ?? '', /timeout-ms/);
    assert.match(validateWatchFlags({ 'timeout-ms': '0' }) ?? '', /timeout-ms/);
    assert.match(validateWatchFlags({ 'timeout-ms': true }) ?? '', /timeout-ms/);
  });
});

describe('puppet-master watch (CLI process)', () => {
  const entry = join(dirname(fileURLToPath(import.meta.url)), 'index.ts');
  const run = (...args: string[]) =>
    spawnSync(process.execPath, ['--import', 'tsx', entry, 'watch', ...args], {
      encoding: 'utf8',
      timeout: 30_000,
    });

  it('exits 4 with a usage message for an unknown flag, without contacting the bridge', () => {
    const result = run('some-operation', '--timeout', '5');
    assert.equal(result.status, 4, result.stderr);
    assert.match(result.stderr, /unknown flag/i);
    assert.match(result.stderr, /--timeout-ms/);
  });
});
