import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  BRANCH_CHANGED_EVENT,
  branchBrief,
  branchLineage,
  branchKeepWarning,
  branchPreview,
  branchStart,
  branchStateText,
  onBranchChanged,
  type BranchFrom,
  type BranchPreview,
  type BranchView,
} from './branch-from';

const view = (patch: Partial<BranchView> = {}): BranchView => ({
  id: 'bf-0123456789ab',
  fromLabel: 'Fake A',
  toLabel: 'Fake B · Work',
  fromPaneId: 'pane-a',
  toPaneId: 'pane-b',
  atMs: 1_000,
  pointLabel: 'after turn 5 · 14:02',
  state: 'running',
  stateText: 'Running in a separate copy',
  turns: 5,
  briefBytes: 321,
  branchName: 'pm/bf-0123456789ab',
  closed: null,
  failedFiles: 0,
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

describe('branch from here command arguments', () => {
  it('sends a chat turn as a snake_case message shape with camelCase argument names', async () => {
    const from: BranchFrom = { kind: 'message', user_item_id: 'u-1', when: 'before' };
    const preview: BranchPreview = {
      fromLabel: 'Fake A',
      point: { checkpointId: '5', label: 'Before turn 6', atMs: 1_000, files: 2 },
      canBranch: true,
      whyNot: null,
      offers: [],
      prefill: 'Try again',
      turns: 5,
      openBranches: 0,
      brief: null,
    };
    invoke.mockResolvedValueOnce(preview);
    await expect(branchPreview('pane-a', from)).resolves.toEqual(preview);
    expect(invoke).toHaveBeenCalledWith('branch_preview', {
      paneId: 'pane-a',
      from: { kind: 'message', user_item_id: 'u-1', when: 'before' },
      target: null,
      message: null,
    });
  });

  it('passes the target and message to the preview when given', async () => {
    invoke.mockResolvedValueOnce({});
    await branchPreview('pane-a', { kind: 'message', user_item_id: 'u-2', when: 'after' }, 'fake_b@work', 'Try a test');
    expect(invoke).toHaveBeenCalledWith('branch_preview', {
      paneId: 'pane-a',
      from: { kind: 'message', user_item_id: 'u-2', when: 'after' },
      target: 'fake_b@work',
      message: 'Try a test',
    });
  });

  it('sends a History row as a save point shape with its digits id', async () => {
    const from: BranchFrom = { kind: 'save_point', checkpoint_id: '12' };
    invoke.mockResolvedValueOnce({});
    await branchPreview('pane-a', from, 'fake_a');
    expect(invoke).toHaveBeenCalledWith('branch_preview', {
      paneId: 'pane-a',
      from: { kind: 'save_point', checkpoint_id: '12' },
      target: 'fake_a',
      message: null,
    });
  });

  it('starts a branch with the target and the person message', async () => {
    const started = view({ state: 'preparing', stateText: 'Making a copy from the save point' });
    invoke.mockResolvedValueOnce(started);
    await expect(
      branchStart('pane-a', { kind: 'save_point', checkpoint_id: '7' }, 'fake_b@work', 'Fix the test'),
    ).resolves.toEqual(started);
    expect(invoke).toHaveBeenCalledWith('branch_start', {
      paneId: 'pane-a',
      from: { kind: 'save_point', checkpoint_id: '7' },
      target: 'fake_b@work',
      message: 'Fix the test',
    });
  });

  it('reads the lineage of a pane in the desktop window', async () => {
    const lineage = { from: null, children: [view()] };
    invoke.mockResolvedValueOnce(lineage);
    await expect(branchLineage('pane-a')).resolves.toEqual(lineage);
    expect(invoke).toHaveBeenCalledWith('branch_lineage', { paneId: 'pane-a' });
  });

  it('gives an empty lineage and calls nothing outside the desktop window', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    await expect(branchLineage('pane-a')).resolves.toEqual({ from: null, children: [] });
    expect(invoke).not.toHaveBeenCalled();
  });

  it('reads the stored brief by its id', async () => {
    invoke.mockResolvedValueOnce('Branch: you are starting a new line of work.');
    await expect(branchBrief('bf-0123456789ab')).resolves.toBe('Branch: you are starting a new line of work.');
    expect(invoke).toHaveBeenCalledWith('branch_brief', { id: 'bf-0123456789ab' });
  });
});

describe('branch changed event', () => {
  it('uses the branch://changed event name', () => {
    expect(BRANCH_CHANGED_EVENT).toBe('branch://changed');
  });

  it('forwards the pane ids of a change', async () => {
    const unlisten = vi.fn();
    listen.mockResolvedValueOnce(unlisten);
    const cb = vi.fn();
    await expect(onBranchChanged(cb)).resolves.toBe(unlisten);
    expect(listen).toHaveBeenCalledWith('branch://changed', expect.any(Function));
    const handler = listen.mock.calls[0][1] as (event: { payload: { paneIds: string[] } }) => void;
    handler({ payload: { paneIds: ['pane-a', 'pane-b'] } });
    expect(cb).toHaveBeenCalledWith({ paneIds: ['pane-a', 'pane-b'] });
  });

  it('listens to nothing outside the desktop window', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    const unlisten = await onBranchChanged(vi.fn());
    expect(listen).not.toHaveBeenCalled();
    expect(() => unlisten()).not.toThrow();
  });
});

describe('branchStateText', () => {
  it('keeps the server text when there is one', () => {
    expect(branchStateText(view({ stateText: 'Branch couldn\'t start: Fake B is out' }))).toBe(
      'Branch couldn\'t start: Fake B is out',
    );
  });

  it('falls back to a plain line for each state when the server text is empty', () => {
    expect(branchStateText(view({ state: 'preparing', stateText: '' }))).toBe('Making a copy from the save point');
    expect(branchStateText(view({ state: 'starting', stateText: '  ' }))).toBe('Fake B · Work is starting');
    expect(branchStateText(view({ state: 'running', stateText: '' }))).toBe('Running in a separate copy');
    expect(branchStateText(view({ state: 'failed', stateText: '' }))).toBe("Fake B · Work couldn't start");
  });
});

describe('branchKeepWarning', () => {
  it('says nothing for a normal copy (no branch)', () => {
    expect(branchKeepWarning(null)).toBeNull();
  });

  it('says nothing once the branch is kept or thrown away', () => {
    expect(branchKeepWarning(view({ closed: 'kept' }))).toBeNull();
    expect(branchKeepWarning(view({ closed: 'thrown_away' }))).toBeNull();
  });

  it('warns on an open branch, with its save point, in plain words', () => {
    expect(branchKeepWarning(view())).toBe(
      'This branch started after turn 5 · 14:02. The original chat may have kept working after that, and merging this branch does not bring that later work in. Check both chats before you merge.',
    );
  });

  it('keeps a sentence when the save point label is empty', () => {
    expect(branchKeepWarning(view({ pointLabel: ' ' }))).toContain('This branch started from a save point.');
  });
});
