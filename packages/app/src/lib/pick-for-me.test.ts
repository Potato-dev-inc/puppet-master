import { afterEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import {
  EARLY_TITLE,
  FOOTER,
  type PickResult,
  isEarly,
  pickHint,
  startLabel,
  startable,
  tauriPickApi,
} from './pick-for-me';

const PROJECT = 'C:\\Users\\you\\work\\app';

function result(over: Partial<PickResult> = {}): PickResult {
  return {
    choice: { agentId: 'fake_a', label: 'Fake A' },
    reason: 'Only Fake A is ready.',
    basis: 'only_one',
    metric: null,
    sample: 'none',
    considered: [{ label: 'Fake A', state: 'ready', line: 'No runs here yet' }],
    notes: ['Counted from your own turns in this project, last 90 days.'],
    ...over,
  };
}

afterEach(() => {
  invoke.mockReset();
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

describe('tauriPickApi', () => {
  it('refuses outside the desktop shell and never calls invoke', async () => {
    await expect(tauriPickApi.pick(PROJECT)).rejects.toThrow('Desktop shell required: pick_agent');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('calls pick_agent with the folder only, and returns the answer', async () => {
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
    const answer = result();
    invoke.mockResolvedValue(answer);

    await expect(tauriPickApi.pick(PROJECT)).resolves.toBe(answer);

    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(command).toBe('pick_agent');
    // The privacy promise: the only argument is the folder. No task, no prompt, no other key.
    expect(Object.keys(args)).toEqual(['project']);
    expect(args).toEqual({ project: PROJECT });
  });

  it('lets a rejected call reach the caller', async () => {
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockRejectedValue(new Error('INVALID_ARGUMENT: project is empty'));
    await expect(tauriPickApi.pick('')).rejects.toThrow('INVALID_ARGUMENT');
  });
});

describe('pickHint', () => {
  it('says Looking... while the call runs', () => {
    expect(pickHint('loading')).toBe('Looking...');
  });

  it('shows the label of the chosen agent', () => {
    expect(pickHint(result())).toBe('Fake A');
  });

  it('says None ready when nothing can run', () => {
    expect(pickHint(result({ choice: null, basis: 'none_ready', considered: [] }))).toBe('None ready');
  });
});

describe('isEarly', () => {
  it('is true only for an early sample (5 to 9 runs)', () => {
    expect(isEarly(result({ sample: 'early' }))).toBe(true);
    expect(isEarly(result({ sample: 'steady' }))).toBe(false);
    expect(isEarly(result({ sample: 'not_enough' }))).toBe(false);
    expect(isEarly(result({ sample: 'none' }))).toBe(false);
  });

  it('is false for a too-early basis with no early sample', () => {
    expect(isEarly(result({ basis: 'too_early', sample: 'not_enough' }))).toBe(false);
  });
});

describe('startLabel', () => {
  it('names the chosen agent', () => {
    expect(startLabel(result())).toBe('Start Fake A');
  });

  it('is plain Start when there is no choice', () => {
    expect(startLabel(result({ choice: null, basis: 'none_ready' }))).toBe('Start');
  });
});

describe('startable', () => {
  const entries = [
    { type: 'fake_a', label: 'Fake A' },
    { type: 'fake_b', label: 'Fake B', disabled: true },
  ];

  it('is true when the chosen agent is in the menu and enabled', () => {
    expect(startable(result(), entries)).toBe(true);
  });

  it('is false when the chosen agent is not in the menu', () => {
    expect(startable(result({ choice: { agentId: 'fake_c', label: 'Fake C' } }), entries)).toBe(false);
  });

  it('is false when the chosen agent is disabled in the menu', () => {
    expect(startable(result({ choice: { agentId: 'fake_b', label: 'Fake B' } }), entries)).toBe(false);
  });

  it('is false when there is no choice', () => {
    expect(startable(result({ choice: null, basis: 'none_ready' }), entries)).toBe(false);
  });

  it('is false for an empty menu', () => {
    expect(startable(result(), [])).toBe(false);
  });
});

describe('fixed text', () => {
  it('has the early title and the footer in plain words', () => {
    expect(EARLY_TITLE).toBe('Based on only a few runs. It may change.');
    expect(FOOTER).toBe('Worked out on your computer. Nothing you type is sent anywhere.');
  });
});
