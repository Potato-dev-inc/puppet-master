// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import type { ChecksReport, PrPrepared, SetupReport } from './ship-api';
import { cannotCreate, checksSentence, copyCommands, copyDefault, lastRealFolder, projectOfCwd, readCopyPreference, realRecents, readStoredCopyPreference, setupLine, stateHint, supportsCopies, writeCopyPreference } from './ship-model';

const report = (state: ChecksReport['state'], items: ChecksReport['items']): ChecksReport => ({ state, items, checked_at_ms: 1 });

describe('checksSentence', () => {
  it('names the failing checks', () => {
    const r = report('failing', [
      { name: 'test (ubuntu)', state: 'fail' },
      { name: 'lint', state: 'fail' },
      { name: 'build', state: 'pass' },
    ]);
    expect(checksSentence(r)).toBe('2 GitHub checks failed: test (ubuntu), lint');
    expect(checksSentence(report('failing', [{ name: 'lint', state: 'fail' }]))).toBe('1 GitHub check failed: lint');
  });
  it('says running, passed, none and unknown', () => {
    expect(checksSentence(report('pending', [{ name: 'a', state: 'pending' }, { name: 'b', state: 'pass' }]))).toBe('GitHub checks are running (1 left)');
    expect(checksSentence(report('passing', [{ name: 'a', state: 'pass' }, { name: 'b', state: 'skipped' }]))).toBe('All 2 GitHub checks passed');
    expect(checksSentence(report('none', []))).toBe('GitHub has no checks for this pull request yet');
    expect(checksSentence(undefined)).toBe('Checking GitHub…');
  });
});

const prepared = (over: Partial<PrPrepared> = {}): PrPrepared => ({
  gh: 'ready',
  has_remote: true,
  on_github: true,
  default_branch: 'main',
  existing: null,
  title: 't',
  body: 'b',
  base: 'main',
  branch: 'pm/x',
  agent: null,
  has_uncommitted: false,
  ...over,
});

describe('cannotCreate', () => {
  it('is null when everything is ready', () => {
    expect(cannotCreate(prepared())).toBeNull();
  });
  it('explains a missing gh, a signed-out gh and a project that is not on GitHub', () => {
    expect(cannotCreate(prepared({ gh: 'missing' }))?.text).toContain('GitHub command line');
    expect(cannotCreate(prepared({ gh: 'signed_out' }))).toEqual({ text: expect.stringContaining('Sign in to GitHub first'), command: 'gh auth login' });
    expect(cannotCreate(prepared({ on_github: false }))?.text).toContain("isn't on GitHub");
  });
  it('a project with no GitHub remote is told so, and gets the commands for when it adds one', () => {
    const blocked = cannotCreate(prepared({ has_remote: false, on_github: false }));
    expect(blocked?.text).toContain('no GitHub remote');
    expect(blocked?.text).toContain('Merge into the main branch');
    expect(blocked?.byHand).toBe(true);
    expect(blocked?.heading).toContain('git remote add origin');
    expect(blocked?.text.length).toBeLessThan(200);
  });
});

describe('copyCommands', () => {
  it('pushes the branch and opens the draft, without a force flag', () => {
    const text = copyCommands('pm/x', 'main', 'Say "hi"');
    expect(text).toBe(`git push --set-upstream origin pm/x\ngh pr create --draft --base main --head pm/x --title "Say 'hi'"`);
    expect(text).not.toMatch(/--force|-f\b/);
  });
});

describe('setupLine', () => {
  const base: SetupReport = { status: 'ok', copied: ['.env'], not_copied: [], steps: [{ command: 'npm ci', ok: true, exit_code: 0, timed_out: false, duration_ms: 1 }], duration_ms: 1 };
  it('reads like the receipt sentence', () => {
    expect(setupLine(base)).toBe('Ran npm ci; copied .env (never committed).');
    expect(setupLine(null)).toBeNull();
  });
  it('says a failure and a time-out in plain words', () => {
    const failed: SetupReport = { ...base, status: 'failed', copied: [], steps: [{ command: 'npm ci', ok: false, exit_code: 1, timed_out: false, duration_ms: 1 }] };
    expect(setupLine(failed)).toBe('Setting up the copy failed: npm ci stopped with an error.');
    const slow: SetupReport = { ...failed, steps: [{ ...failed.steps[0], timed_out: true }] };
    expect(setupLine(slow)).toContain('took too long');
  });
});

describe('projects', () => {
  it('maps a separate copy back to its project', () => {
    expect(projectOfCwd('C:\\code\\shop\\.puppet-master\\worktrees\\run-1-ab12')).toEqual({ path: 'C:\\code\\shop', name: 'shop' });
    expect(projectOfCwd('/home/me/shop')).toEqual({ path: '/home/me/shop', name: 'shop' });
    expect(projectOfCwd(undefined)).toBeNull();
  });
  it('only a Git project with a first snapshot can make copies', () => {
    expect(supportsCopies('repo')).toBe(true);
    expect(supportsCopies(undefined)).toBe(true);
    expect(supportsCopies('folder')).toBe(false);
    expect(supportsCopies('no_commits')).toBe(false);
    expect(stateHint('missing')).toBe('Folder not found');
    expect(stateHint('repo')).toBe('');
  });
  describe('Separate copy is remembered per project', () => {
    beforeEach(() => localStorage.clear());
    it('keeps one answer for each folder', () => {
      expect(readCopyPreference('C:\\a')).toBe(false);
      writeCopyPreference('C:\\a', true);
      expect(readCopyPreference('C:\\a')).toBe(true);
      expect(readCopyPreference('C:\\b')).toBe(false);
      writeCopyPreference('C:\\a', false);
      expect(readCopyPreference('C:\\a')).toBe(false);
    });
  });
  describe('Separate copy starts OFF (the selected folder is the folder)', () => {
    beforeEach(() => localStorage.clear());
    it('is off when nothing was chosen', () => {
      expect(copyDefault(undefined)).toBe(false);
    });
    it("the person's remembered choice wins, either way", () => {
      expect(copyDefault(false)).toBe(false);
      expect(copyDefault(true)).toBe(true);
    });
    it('reads a project nobody chose for as unset, not as a stored answer', () => {
      expect(readStoredCopyPreference('C:\never')).toBeUndefined();
      expect(copyDefault(readStoredCopyPreference('C:\never'))).toBe(false);
    });
    it('keeps an explicit ON for the project it was chosen for, and only that one', () => {
      writeCopyPreference('C:\a', true);
      expect(copyDefault(readStoredCopyPreference('C:\a'))).toBe(true);
      expect(copyDefault(readStoredCopyPreference('C:\b'))).toBe(false);
    });
    it('tells "never chose" from "chose off"', () => {
      expect(readStoredCopyPreference('C:\\a')).toBeUndefined();
      writeCopyPreference('C:\\a', false);
      expect(readStoredCopyPreference('C:\\a')).toBe(false);
    });
  });
  describe('recent projects never name a separate copy', () => {
    const at = (path: string, last_used_ms = 1, state: 'repo' | 'missing' = 'repo') => ({ path, name: path.split(/[\\/]/).pop()!, state, last_used_ms });
    const COPY = 'C:\\code\\shop\\.puppet-master\\worktrees\\pane-1a2b';
    it('maps a copy back to its project and lists each folder once', () => {
      const listed = realRecents([at(COPY, 5), at('C:\\code\\shop', 4), at('C:\\code\\site', 3)]);
      expect(listed.map((p) => p.path)).toEqual(['C:\\code\\shop', 'C:\\code\\site']);
      expect(listed[0].name).toBe('shop');
      expect(JSON.stringify(listed)).not.toContain('.puppet-master');
    });
    it('"+" defaults to the last real folder: the newest one the person used, never a copy or a missing folder', () => {
      expect(lastRealFolder([])).toBeNull();
      expect(lastRealFolder([at('C:\\code\\gone', 9, 'missing'), at(COPY, 8), at('C:\\code\\site', 7)])).toEqual({ path: 'C:\\code\\shop', name: 'shop', state: 'repo' });
      // A folder that only ever had runs (last used 0) is not "the last folder the person used".
      expect(lastRealFolder([at('C:\\code\\runs-only', 0)])).toBeNull();
    });
  });
});
