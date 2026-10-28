import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  KEEP_GOING_CHANGED_EVENT,
  keepGoingBrief,
  keepGoingContinue,
  keepGoingLineage,
  keepGoingSettingsGet,
  keepGoingSettingsSet,
  keepGoingTargetKey,
  keepGoingTargets,
  onKeepGoingChanged,
  stateText,
  type HandoverView,
  type KeepGoingOffer,
  type KeepGoingSettings,
} from './keep-going';

const view = (patch: Partial<HandoverView> = {}): HandoverView => ({
  id: 'kg-0123456789ab',
  fromLabel: 'Fake A',
  toLabel: 'Fake B · Work',
  fromPaneId: 'pane-a',
  toPaneId: 'pane-b',
  atMs: 1_000,
  state: 'running',
  stateText: 'Continued on Fake B · Work',
  turns: 14,
  briefBytes: 321,
  backAtMs: null,
  reason: 'usage_limit',
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

describe('keep going command arguments', () => {
  it('sends the pane id in camelCase for the picker rows', async () => {
    const rows: KeepGoingOffer[] = [
      { target: { agent_id: 'fake_b' }, label: 'Fake B', ok: true, whyNot: null },
      { target: { agent_id: 'fake_a', account: 'work' }, label: 'Fake A · Work', ok: false, whyNot: 'Same company as Fake A' },
    ];
    invoke.mockResolvedValueOnce(rows);
    await expect(keepGoingTargets('pane-a')).resolves.toEqual(rows);
    expect(invoke).toHaveBeenCalledWith('keep_going_targets', { paneId: 'pane-a' });
  });

  it('passes the target text, the move-back switch and the card id', async () => {
    invoke.mockResolvedValueOnce(view());
    await keepGoingContinue('pane-a', 'fake_b@work', true, 'card-1');
    expect(invoke).toHaveBeenCalledWith('keep_going_continue', {
      paneId: 'pane-a',
      target: 'fake_b@work',
      back: true,
      cardId: 'card-1',
    });
  });

  it('sends a null card id when none is given', async () => {
    invoke.mockResolvedValueOnce(view());
    await keepGoingContinue('pane-a', 'fake_b', false);
    expect(invoke).toHaveBeenCalledWith('keep_going_continue', {
      paneId: 'pane-a',
      target: 'fake_b',
      back: false,
      cardId: null,
    });
  });

  it('returns the stopping record from continue', async () => {
    const started = view({ state: 'stopping', stateText: 'Moving to Fake B · Work' });
    invoke.mockResolvedValueOnce(started);
    await expect(keepGoingContinue('pane-a', 'fake_b', true)).resolves.toEqual(started);
  });

  it('reads the brief by its record id', async () => {
    invoke.mockResolvedValueOnce('Continue: fix the cart total');
    await expect(keepGoingBrief('kg-0123456789ab')).resolves.toBe('Continue: fix the cart total');
    expect(invoke).toHaveBeenCalledWith('keep_going_brief', { id: 'kg-0123456789ab' });
  });

  it('passes the settings object with its snake_case fields', async () => {
    const settings: KeepGoingSettings = { order: [{ agent_id: 'fake_b' }, { agent_id: 'fake_c', account: 'work' }], ask_back: false };
    invoke.mockResolvedValueOnce(settings);
    await expect(keepGoingSettingsSet(settings)).resolves.toEqual(settings);
    expect(invoke).toHaveBeenCalledWith('keep_going_settings_set', { settings });
  });

  it('reads the settings with no arguments', async () => {
    invoke.mockResolvedValueOnce({ order: [], ask_back: true });
    await expect(keepGoingSettingsGet()).resolves.toEqual({ order: [], ask_back: true });
    expect(invoke).toHaveBeenCalledWith('keep_going_settings_get');
  });
});

describe('keepGoingLineage', () => {
  it('returns both sides from the desktop command', async () => {
    const lineage = { from: view({ toPaneId: 'pane-a' }), to: null };
    invoke.mockResolvedValueOnce(lineage);
    await expect(keepGoingLineage('pane-a')).resolves.toEqual(lineage);
    expect(invoke).toHaveBeenCalledWith('keep_going_lineage', { paneId: 'pane-a' });
  });

  it('reads as no hand-overs outside the desktop window', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    await expect(keepGoingLineage('pane-a')).resolves.toEqual({ from: null, to: null });
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('onKeepGoingChanged', () => {
  it('listens to the keep-going changed event and hands over the pane ids', async () => {
    const unlisten = vi.fn();
    listen.mockImplementationOnce(async (_name: string, handler: (e: { payload: unknown }) => void) => {
      handler({ payload: { paneIds: ['pane-a', 'pane-b'] } });
      return unlisten;
    });
    const cb = vi.fn();
    const off = await onKeepGoingChanged(cb);
    expect(listen).toHaveBeenCalledWith(KEEP_GOING_CHANGED_EVENT, expect.any(Function));
    expect(KEEP_GOING_CHANGED_EVENT).toBe('keep-going://changed');
    expect(cb).toHaveBeenCalledWith({ paneIds: ['pane-a', 'pane-b'] });
    off();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('does not listen outside the desktop window', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    const off = await onKeepGoingChanged(vi.fn());
    expect(listen).not.toHaveBeenCalled();
    expect(() => off()).not.toThrow();
  });
});

describe('keepGoingTargetKey', () => {
  it('gives agent or agent@account, as the Rust side reads it', () => {
    expect(keepGoingTargetKey({ agent_id: 'fake_b' })).toBe('fake_b');
    expect(keepGoingTargetKey({ agent_id: 'fake_b', account: 'work' })).toBe('fake_b@work');
  });
});

describe('stateText', () => {
  it('uses the text the desktop sent', () => {
    expect(stateText(view({ stateText: 'Continued on Fake B · Work' }))).toBe('Continued on Fake B · Work');
    expect(stateText(view({ state: 'failed', stateText: '  Fake B couldn\'t start  ' }))).toBe("Fake B couldn't start");
  });

  it('falls back to plain words for each state when the text is empty', () => {
    expect(stateText(view({ state: 'running', stateText: '' }))).toBe('Continued on Fake B · Work');
    expect(stateText(view({ state: 'stopping', stateText: ' ' }))).toBe('Moving to Fake B · Work');
    expect(stateText(view({ state: 'starting', stateText: '' }))).toBe('Moving to Fake B · Work');
    expect(stateText(view({ state: 'failed', stateText: '' }))).toBe("Fake B · Work couldn't start");
  });
});
