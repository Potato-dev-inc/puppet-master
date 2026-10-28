import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { clipFileName, clipPlan, clipSave, clipStage } from './clip';

const setTauri = (on: boolean) => {
  if (on) (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  else delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
};

describe('clip API wrappers', () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(42);
    setTauri(true);
  });
  afterEach(() => setTauri(false));

  it('clipPlan sends the pane and the optional turn id, null when none', async () => {
    await clipPlan('pane-1', 'item-9');
    expect(invoke.mock.calls[0].slice(0, 2)).toEqual(['clip_plan', { paneId: 'pane-1', userItemId: 'item-9' }]);
    await clipPlan('pane-1');
    expect(invoke.mock.calls[1].slice(0, 2)).toEqual(['clip_plan', { paneId: 'pane-1', userItemId: null }]);
  });

  it('clipStage sends the raw bytes with the token and ext in headers', async () => {
    const bytes = new Uint8Array([0, 0, 0, 24, 0x66, 0x74, 0x79, 0x70]);
    const kept = await clipStage('tok-abc', 'mp4', bytes);
    expect(kept).toBe(42);
    expect(invoke).toHaveBeenCalledWith('clip_stage', bytes, {
      headers: { 'x-clip-token': 'tok-abc', 'x-clip-ext': 'mp4' },
    });
  });

  it('clipStage passes webm as the ext header', async () => {
    await clipStage('tok-abc', 'webm', new Uint8Array(4));
    expect(invoke.mock.calls[0][2]).toEqual({ headers: { 'x-clip-token': 'tok-abc', 'x-clip-ext': 'webm' } });
  });

  it('clipSave sends the token and the chosen path', async () => {
    invoke.mockResolvedValue('D:\\out\\clip.mp4');
    expect(await clipSave('tok-abc', 'D:\\out\\clip.mp4')).toBe('D:\\out\\clip.mp4');
    expect(invoke.mock.calls[0].slice(0, 2)).toEqual(['clip_save', { token: 'tok-abc', path: 'D:\\out\\clip.mp4' }]);
  });

  it('refuses every call when the app is not the desktop shell', async () => {
    setTauri(false);
    await expect(clipPlan('pane-1')).rejects.toThrow('Desktop shell required: clip_plan');
    await expect(clipStage('t', 'mp4', new Uint8Array(1))).rejects.toThrow('Desktop shell required: clip_stage');
    await expect(clipSave('t', 'x.mp4')).rejects.toThrow('Desktop shell required: clip_save');
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('clipFileName', () => {
  it('uses only the date and the time, with mp4 by default', () => {
    expect(clipFileName(new Date(2026, 9, 28, 15, 30))).toBe('clip-2026-10-28-1530.mp4');
  });

  it('pads single digits and takes the webm extension', () => {
    expect(clipFileName(new Date(2026, 0, 3, 7, 5), 'webm')).toBe('clip-2026-01-03-0705.webm');
  });
});
