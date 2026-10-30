// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  type CiFixSettings,
  type CiLogNotes,
  type WatchView,
  ciFixAct,
  ciFixLog,
  ciFixOffer,
  ciFixPollNow,
  ciFixSaveSettings,
  ciFixView,
  notesLine,
  onCiFixChanged,
  watchLine,
} from './ci-fix';

const settings: CiFixSettings = {
  enabled: true,
  tries_per_pr: 2,
  turns_per_try: 15,
  minutes_per_try: 20,
  fixes_per_day: 6,
};

const notes: CiLogNotes = {
  lines: 200,
  cut_lines: 0,
  hidden_secrets: 2,
  invisible_chars: 0,
  jobs_read: 1,
  jobs_without_log: 0,
  truncated: false,
};

function watch(over: Partial<WatchView> = {}): WatchView {
  return {
    id: 'cw-1a2b3c4d',
    repo: 'acme/billing',
    prNumber: 61,
    prUrl: 'https://github.com/acme/billing/pull/61',
    branch: 'pm/issue-42-a1b2',
    agentLabel: 'Codex',
    state: 'watching',
    stateText: 'Watching CI',
    triesUsed: 0,
    triesMax: 2,
    lastFix: null,
    project: 'C:/you/projects/billing',
    handle: 'run-1',
    ...over,
  };
}

/** Tauri sets `__TAURI_INTERNALS__` on the window; the wrappers check it before they call invoke. */
function setDesktop(on: boolean): void {
  if (on) (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  else delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
}

describe('ci-fix commands (desktop shell)', () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
    listen.mockReset();
    setDesktop(true);
  });

  afterEach(() => {
    setDesktop(false);
  });

  it('calls each ci_fix command with the argument names Tauri expects', async () => {
    await ciFixView();
    expect(invoke).toHaveBeenLastCalledWith('ci_fix_view');

    await ciFixSaveSettings(settings);
    expect(invoke).toHaveBeenLastCalledWith('ci_fix_save_settings', { settings });

    await ciFixOffer(null, 'run-1');
    expect(invoke).toHaveBeenLastCalledWith('ci_fix_offer', { projectPath: null, handle: 'run-1' });

    await ciFixOffer('C:/you/projects/billing', 'run-1');
    expect(invoke).toHaveBeenLastCalledWith('ci_fix_offer', {
      projectPath: 'C:/you/projects/billing',
      handle: 'run-1',
    });

    await ciFixAct('cw-1a2b3c4d', 'stop_watching');
    expect(invoke).toHaveBeenLastCalledWith('ci_fix_act', { id: 'cw-1a2b3c4d', action: 'stop_watching' });

    await ciFixLog('cf-0123456789ab');
    expect(invoke).toHaveBeenLastCalledWith('ci_fix_log', { fixId: 'cf-0123456789ab' });

    await ciFixPollNow();
    expect(invoke).toHaveBeenLastCalledWith('ci_fix_poll_now');
  });

  it('passes the backend answer through unchanged', async () => {
    const view = { settings, gh: { state: 'ready', login: 'you' }, watches: [watch()], fixes: [] };
    invoke.mockResolvedValueOnce(view);
    await expect(ciFixView()).resolves.toEqual(view);

    invoke.mockResolvedValueOnce(watch());
    await expect(ciFixOffer(null, 'run-1')).resolves.toMatchObject({ prNumber: 61, agentLabel: 'Codex' });
  });

  it('refuses outside the desktop shell and never calls invoke', async () => {
    setDesktop(false);
    await expect(ciFixView()).rejects.toThrow('Desktop shell required: ci_fix_view');
    await expect(ciFixAct('cw-1', 'push')).rejects.toThrow('Desktop shell required: ci_fix_act');
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('onCiFixChanged', () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
  });

  afterEach(() => {
    setDesktop(false);
  });

  it('listens to ci-fix://changed and calls the callback when it fires', async () => {
    setDesktop(true);
    const unlisten = vi.fn();
    listen.mockResolvedValueOnce(unlisten);
    const cb = vi.fn();

    const stop = await onCiFixChanged(cb);

    expect(listen).toHaveBeenCalledWith('ci-fix://changed', expect.any(Function));
    const handler = listen.mock.calls[0][1] as () => void;
    handler();
    expect(cb).toHaveBeenCalledTimes(1);
    expect(stop).toBe(unlisten);
  });

  it('does nothing outside the desktop shell', async () => {
    setDesktop(false);
    const cb = vi.fn();

    const stop = await onCiFixChanged(cb);

    expect(listen).not.toHaveBeenCalled();
    expect(cb).not.toHaveBeenCalled();
    expect(() => stop()).not.toThrow();
  });
});

describe('watchLine', () => {
  it('joins the PR number, repo, agent and row text', () => {
    expect(watchLine(watch())).toBe('#61 acme/billing · Codex · Watching CI');
  });

  it('shows the backend row text as it is', () => {
    expect(watchLine(watch({ state: 'merged', stateText: 'CI passed after 1 fix' }))).toBe(
      '#61 acme/billing · Codex · CI passed after 1 fix',
    );
  });
});

describe('notesLine', () => {
  it('says the line count and the hidden secrets', () => {
    expect(notesLine(notes)).toBe('200 log lines · 2 secrets hidden');
  });

  it('uses singular words for one line, one secret and one invisible character', () => {
    expect(notesLine({ ...notes, lines: 1, hidden_secrets: 1, invisible_chars: 1 })).toBe(
      '1 log line · 1 secret hidden · 1 invisible character removed',
    );
  });

  it('adds the invisible characters only when some were removed', () => {
    expect(notesLine({ ...notes, hidden_secrets: 0, invisible_chars: 3 })).toBe(
      '200 log lines · 3 invisible characters removed',
    );
    expect(notesLine({ ...notes, hidden_secrets: 0, invisible_chars: 0 })).toBe('200 log lines');
  });

  it('handles an empty log', () => {
    expect(notesLine({ ...notes, lines: 0, hidden_secrets: 0 })).toBe('0 log lines');
  });
});
