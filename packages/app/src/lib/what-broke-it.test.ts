import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  WHAT_BROKE_IT_CHANGED_EVENT,
  kindOf,
  onWhatBrokeItChanged,
  wbDeclare,
  wbDiff,
  wbFinding,
  wbFindings,
  wbOverview,
  wbRecheck,
  wbSettingsGet,
  wbSettingsSet,
  wbStart,
  wbStatus,
  wbStop,
  wbUndo,
  wbUndoPlan,
  type FindingKind,
  type FindingView,
  type JobView,
  type Limits,
  type Overview,
} from './what-broke-it';

const job = (patch: Partial<JobView> = {}): JobView => ({
  id: 'wb-job-1',
  state: 'running',
  pointNo: 4,
  points: 9,
  run: 3,
  maxRuns: 16,
  elapsedMs: 120_000,
  maxMs: 1_200_000,
  line: 'Trying save point 4 of 9 · run 3 of 16 · 2 min',
  findingId: null,
  ...patch,
});

const finding = (patch: Partial<FindingView> = {}): FindingView => ({
  id: 'wb-0123456789ab',
  title: 'Turn 7 by Codex broke the tests',
  subtitle: '14:10 · "refactor cart" · 4 files, +52 −20',
  kind: 'culprit',
  suspects: [
    {
      paneKey: 'pane-a',
      turn: 7,
      agentLabel: 'Codex',
      agentId: 'codex',
      account: null,
      atMs: 1_000,
      sentence: 'Refactored the cart.',
      files: { count: 4, added: 52, deleted: 20 },
    },
  ],
  failing: ['cart.test.ts'],
  tail: 'FAIL cart.test.ts',
  canUndo: true,
  undoneAtMs: null,
  ...patch,
});

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe('what broke it command arguments', () => {
  it('sends the pane id and the declared test fields in camelCase for the overview and declare', async () => {
    const overview: Overview = {
      tests: [{ index: 0, name: 'npm test', command_text: 'npm test', timeout_min: 5, warnings: [] }],
      suggestions: [],
      points: [{ checkpointId: 'cp-1', label: 'Codex · turn 7 · 14:10', atMs: 1_000 }],
      limits: { max_runs: 16, max_minutes: 20 },
      can: true,
      whyNot: null,
      busy: null,
    };
    invoke.mockResolvedValueOnce(overview);
    await expect(wbOverview('pane-a')).resolves.toEqual(overview);
    expect(invoke).toHaveBeenLastCalledWith('wb_overview', { paneId: 'pane-a' });

    invoke.mockResolvedValueOnce(overview.tests[0]);
    await wbDeclare('pane-a', 'npm test', 'npm test', 5);
    expect(invoke).toHaveBeenLastCalledWith('wb_declare', {
      paneId: 'pane-a',
      name: 'npm test',
      commandText: 'npm test',
      timeoutMin: 5,
    });
  });

  it('sends null for the optional start arguments when they are left out', async () => {
    invoke.mockResolvedValueOnce(job());
    await expect(wbStart('pane-a', 0)).resolves.toEqual(job());
    expect(invoke).toHaveBeenLastCalledWith('wb_start', {
      paneId: 'pane-a',
      checkIndex: 0,
      goodCheckpointId: null,
      limits: null,
      continueFrom: null,
    });
  });

  it('sends the chosen good save point, limits and continue-from when they are given', async () => {
    const limits: Limits = { max_runs: 8, max_minutes: 10 };
    invoke.mockResolvedValueOnce(job());
    await wbStart('pane-a', 2, 'cp-9', limits, 'wb-0123456789ab');
    expect(invoke).toHaveBeenLastCalledWith('wb_start', {
      paneId: 'pane-a',
      checkIndex: 2,
      goodCheckpointId: 'cp-9',
      limits,
      continueFrom: 'wb-0123456789ab',
    });
  });

  it('sends the job id for status and stop', async () => {
    invoke.mockResolvedValueOnce(job({ state: 'done', findingId: 'wb-0123456789ab' }));
    await expect(wbStatus('wb-job-1')).resolves.toMatchObject({ state: 'done', findingId: 'wb-0123456789ab' });
    expect(invoke).toHaveBeenLastCalledWith('wb_status', { jobId: 'wb-job-1' });

    invoke.mockResolvedValueOnce(undefined);
    await expect(wbStop('wb-job-1')).resolves.toBeUndefined();
    expect(invoke).toHaveBeenLastCalledWith('wb_stop', { jobId: 'wb-job-1' });
  });

  it('sends the finding id, and the pane id for the list', async () => {
    invoke.mockResolvedValueOnce(finding());
    await expect(wbFinding('wb-0123456789ab')).resolves.toEqual(finding());
    expect(invoke).toHaveBeenLastCalledWith('wb_finding', { findingId: 'wb-0123456789ab' });

    invoke.mockResolvedValueOnce([finding()]);
    await expect(wbFindings('pane-a')).resolves.toEqual([finding()]);
    expect(invoke).toHaveBeenLastCalledWith('wb_findings', { paneId: 'pane-a' });
  });

  it('sends a null path when the file list is asked for and the path when one file is', async () => {
    const files = [{ path: 'src/cart.ts', status: 'M', added: 3, deleted: 1, binary: false }];
    invoke.mockResolvedValueOnce({ files, patch: null, truncated: false });
    await expect(wbDiff('wb-0123456789ab')).resolves.toEqual({ files, patch: null, truncated: false });
    expect(invoke).toHaveBeenLastCalledWith('wb_diff', { findingId: 'wb-0123456789ab', path: null });

    invoke.mockResolvedValueOnce({ files, patch: '@@ -1 +1 @@', truncated: false });
    await wbDiff('wb-0123456789ab', 'src/cart.ts');
    expect(invoke).toHaveBeenLastCalledWith('wb_diff', { findingId: 'wb-0123456789ab', path: 'src/cart.ts' });
  });

  it('sends the finding id to the undo plan, the undo and the recheck', async () => {
    const plan = { writes: ['src/cart.ts'], removes: [], conflicts: [], blockedBy: null };
    invoke.mockResolvedValueOnce(plan);
    await expect(wbUndoPlan('wb-0123456789ab')).resolves.toEqual(plan);
    expect(invoke).toHaveBeenLastCalledWith('wb_undo_plan', { findingId: 'wb-0123456789ab' });

    invoke.mockResolvedValueOnce({ undoneAtMs: 2_000, written: 1, removed: 0 });
    await expect(wbUndo('wb-0123456789ab')).resolves.toEqual({ undoneAtMs: 2_000, written: 1, removed: 0 });
    expect(invoke).toHaveBeenLastCalledWith('wb_undo', { findingId: 'wb-0123456789ab' });

    invoke.mockResolvedValueOnce(job());
    await wbRecheck('wb-0123456789ab');
    expect(invoke).toHaveBeenLastCalledWith('wb_recheck', { findingId: 'wb-0123456789ab' });
  });

  it('reads and saves the limits; the saved limits go in as `limits`', async () => {
    const saved: Limits = { max_runs: 30, max_minutes: 60 };
    invoke.mockResolvedValueOnce({ max_runs: 16, max_minutes: 20 });
    await expect(wbSettingsGet()).resolves.toEqual({ max_runs: 16, max_minutes: 20 });
    expect(invoke).toHaveBeenLastCalledWith('wb_settings_get');

    invoke.mockResolvedValueOnce(saved);
    await expect(wbSettingsSet(saved)).resolves.toEqual(saved);
    expect(invoke).toHaveBeenLastCalledWith('wb_settings_set', { limits: saved });
  });
});

describe('what broke it errors', () => {
  it('passes the plain-English error text through unchanged', async () => {
    const message = 'Another check is already running. Wait for it to finish.';
    invoke.mockRejectedValueOnce(message);
    await expect(wbStart('pane-a', 0)).rejects.toBe(message);
  });

  it('passes a refusal from declare through unchanged', async () => {
    const message = "Can't use & in a test command. Type one command, like npm test.";
    invoke.mockRejectedValueOnce(message);
    await expect(wbDeclare('pane-a', 'bad', 'npm test & rm', 5)).rejects.toBe(message);
  });
});

describe('what broke it change event', () => {
  it('uses the backend event name', () => {
    expect(WHAT_BROKE_IT_CHANGED_EVENT).toBe('what-broke-it://changed');
  });

  it('listens to the event, hands the payload to the callback and returns the unlisten function', async () => {
    const unlisten = vi.fn();
    let handler: ((event: { payload: unknown }) => void) | undefined;
    listen.mockImplementationOnce((name: string, cb: (event: { payload: unknown }) => void) => {
      expect(name).toBe('what-broke-it://changed');
      handler = cb;
      return Promise.resolve(unlisten);
    });

    const cb = vi.fn();
    const stop = await onWhatBrokeItChanged(cb);
    handler?.({ payload: { jobId: 'wb-job-1', findingId: 'wb-0123456789ab' } });
    expect(cb).toHaveBeenCalledWith({ jobId: 'wb-job-1', findingId: 'wb-0123456789ab' });

    stop();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('listens to nothing outside the desktop window and returns a no-op unlisten', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    const stop = await onWhatBrokeItChanged(vi.fn());
    expect(listen).not.toHaveBeenCalled();
    expect(() => stop()).not.toThrow();
  });
});

describe('kindOf', () => {
  it.each<[string, FindingKind]>([
    ['culprit', 'culprit'],
    ['several', 'several'],
    ['between_turns', 'between_turns'],
    ['not_sure', 'not_sure'],
    ['always_red', 'always_red'],
    ['good_was_red', 'good_was_red'],
    ['tests_pass', 'tests_pass'],
    ['copy_differs', 'copy_differs'],
    ['failed', 'failed'],
  ])('keeps the known kind %s', (kind, expected) => {
    expect(kindOf(finding({ kind }))).toBe(expected);
  });

  it('falls back to failed for a kind the sheet does not know', () => {
    expect(kindOf(finding({ kind: 'something_new' }))).toBe('failed');
    expect(kindOf(finding({ kind: '' }))).toBe('failed');
  });
});
