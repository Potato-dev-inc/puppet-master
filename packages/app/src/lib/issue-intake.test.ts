import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  type FolderCheck,
  INTAKE_CHANGED_EVENT,
  type IntakeConfig,
  type RepoWatch,
  type RunStateTag,
  type RunView,
  intakeAct,
  intakeCheckFolder,
  intakePollNow,
  intakeSave,
  intakeView,
  onIntakeChanged,
  repoLine,
  stateText,
} from './issue-intake';

function repo(over: Partial<RepoWatch> = {}): RepoWatch {
  return {
    id: 'rw-0a1b2c3d',
    owner_repo: 'acme/billing',
    folder: 'C:\\Users\\you\\code\\billing',
    label: 'puppet',
    agent: { agent_type: 'codex', account: null },
    turns: 30,
    minutes: 45,
    runs_per_day: 5,
    ask_before_start: true,
    ask_before_push: true,
    added_at_ms: 1_000,
    paused: false,
    ...over,
  };
}

function run(over: Partial<RunView> = {}): RunView {
  return {
    id: 'ir-0123456789ab',
    repo: 'acme/billing',
    number: 42,
    title: 'Login button does nothing',
    url: 'https://github.com/acme/billing/issues/42',
    labeledBy: 'you',
    author: 'someone',
    state: 'running',
    stateText: '',
    agentLabel: 'Codex',
    prUrl: null,
    paneId: null,
    atMs: 1_000,
    forced: [],
    ...over,
  };
}

const config: IntakeConfig = {
  enabled: true,
  allowed: [],
  write_access_counts: false,
  parallel: 1,
  repos: [repo()],
  ignored: [['acme/billing', 7]],
};

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe('invoke wrappers', () => {
  it('intakeView calls issue_intake_view with no arguments', async () => {
    invoke.mockResolvedValue({ config, gh: { state: 'ready', login: 'you' }, repos: [], runs: [] });
    const view = await intakeView();
    expect(invoke).toHaveBeenCalledWith('issue_intake_view');
    expect(view.gh.login).toBe('you');
  });

  it('intakeSave sends the whole config under `config`', async () => {
    invoke.mockResolvedValue({ config, gh: { state: 'ready', login: 'you' }, repos: [], runs: [] });
    await intakeSave(config);
    expect(invoke).toHaveBeenCalledWith('issue_intake_save', { config });
  });

  it('intakeCheckFolder sends the folder and returns the check', async () => {
    const check: FolderCheck = { owner_repo: 'acme/billing', default_branch: 'main', gh: 'ready', why_not: null };
    invoke.mockResolvedValue(check);
    await expect(intakeCheckFolder('C:\\Users\\you\\code\\billing')).resolves.toEqual(check);
    expect(invoke).toHaveBeenCalledWith('issue_intake_check_folder', { folder: 'C:\\Users\\you\\code\\billing' });
  });

  it('intakeAct sends the run id and the action in camelCase keys', async () => {
    invoke.mockResolvedValue(run({ state: 'queued' }));
    const out = await intakeAct('ir-0123456789ab', 'start');
    expect(invoke).toHaveBeenCalledWith('issue_intake_act', { runId: 'ir-0123456789ab', action: 'start' });
    expect(out.state).toBe('queued');
  });

  it('intakePollNow calls issue_intake_poll_now', async () => {
    invoke.mockResolvedValue(undefined);
    await intakePollNow();
    expect(invoke).toHaveBeenCalledWith('issue_intake_poll_now');
  });
});

describe('onIntakeChanged', () => {
  it('listens to issue-intake://changed and calls cb without a payload', async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const unlisten = vi.fn();
    let handler: (() => void) | undefined;
    listen.mockImplementation((_name: string, h: () => void) => {
      handler = h;
      return Promise.resolve(unlisten);
    });
    const cb = vi.fn();
    const stop = await onIntakeChanged(cb);
    expect(listen).toHaveBeenCalledWith(INTAKE_CHANGED_EVENT, expect.any(Function));
    expect(INTAKE_CHANGED_EVENT).toBe('issue-intake://changed');
    handler?.();
    expect(cb).toHaveBeenCalledTimes(1);
    expect(stop).toBe(unlisten);
  });

  it('does nothing outside the app', async () => {
    const stop = await onIntakeChanged(vi.fn());
    expect(listen).not.toHaveBeenCalled();
    expect(() => stop()).not.toThrow();
  });
});

describe('stateText', () => {
  it('uses the backend sentence when it sent one', () => {
    expect(stateText(run({ state: 'pr_open', stateText: '  Draft PR #57 opened  ' }))).toBe('Draft PR #57 opened');
  });

  it('falls back to plain text for every state', () => {
    const states: RunStateTag[] = [
      'skipped',
      'asking',
      'declined',
      'ignored',
      'expired',
      'queued',
      'starting',
      'running',
      'checking',
      'ask_push',
      'pushing',
      'pr_open',
      'no_changes',
      'held',
      'thrown_away',
      'failed',
    ];
    for (const state of states) {
      const text = stateText(run({ state }));
      expect(text.length).toBeGreaterThan(0);
    }
    expect(stateText(run({ state: 'declined' }))).toBe('Not now');
    expect(stateText(run({ state: 'pr_open' }))).toBe('Draft PR open');
  });

  it('never uses the words auto, merge or fix it', () => {
    const states: RunStateTag[] = ['skipped', 'asking', 'queued', 'starting', 'running', 'checking', 'ask_push', 'pushing', 'pr_open', 'no_changes', 'held', 'failed'];
    for (const state of states) {
      expect(stateText(run({ state })).toLowerCase()).not.toMatch(/auto|merge|fix it/);
    }
  });
});

describe('repoLine', () => {
  it('joins the repo row in the order Settings shows it', () => {
    expect(repoLine(repo(), 'Codex')).toBe('acme/billing · label puppet · Codex · 30 turns · 45 min · 5 a day');
  });

  it('says "turn" in the singular for one turn', () => {
    expect(repoLine(repo({ turns: 1 }), 'Codex')).toContain(' · 1 turn · ');
  });
});
