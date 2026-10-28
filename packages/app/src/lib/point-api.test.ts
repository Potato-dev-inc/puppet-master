// @vitest-environment jsdom
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { pointApi, type PointState } from './point-api';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

const tauriWindow = () => window as unknown as { __TAURI_INTERNALS__?: unknown };
const emptyState: PointState = { picks: [], targets: [], defaultTarget: null, notice: null };

describe('pointApi outside the desktop shell', () => {
  beforeEach(() => {
    delete tauriWindow().__TAURI_INTERNALS__;
    vi.mocked(invoke).mockReset();
  });

  it('refuses every command without calling invoke', async () => {
    await expect(pointApi.state()).rejects.toThrow('Desktop shell required: point_state');
    await expect(pointApi.openPreview('p1', 5173)).rejects.toThrow('Desktop shell required: point_open_preview');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('listeners do nothing and return a safe unsubscribe', () => {
    const off = pointApi.onState(() => {});
    const offDraft = pointApi.onDraft(() => {});
    expect(listen).not.toHaveBeenCalled();
    expect(() => {
      off();
      offDraft();
    }).not.toThrow();
  });
});

describe('pointApi in the desktop shell', () => {
  beforeEach(() => {
    tauriWindow().__TAURI_INTERNALS__ = {};
    vi.mocked(invoke).mockReset();
    vi.mocked(listen).mockReset();
  });

  afterEach(() => {
    delete tauriWindow().__TAURI_INTERNALS__;
  });

  it('openPreview calls point_open_preview with the pane and port and returns its info', async () => {
    const info = { previewId: 'preview-1', url: 'http://localhost:5173/', title: 'localhost:5173' };
    vi.mocked(invoke).mockResolvedValueOnce(info);
    await expect(pointApi.openPreview('pane-a', 5173)).resolves.toEqual(info);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('point_open_preview', { paneId: 'pane-a', port: 5173 });
  });

  it('openPreview arms the picker after opening only when armed is true', async () => {
    vi.mocked(invoke).mockResolvedValue({ previewId: 'x', url: 'u', title: 't' });
    await pointApi.openPreview('pane-a', 5173, true);
    expect(vi.mocked(invoke).mock.calls.map((c) => c[0])).toEqual(['point_open_preview', 'point_arm']);

    vi.mocked(invoke).mockClear();
    await pointApi.openPreview('pane-a', 5173);
    expect(vi.mocked(invoke).mock.calls.map((c) => c[0])).toEqual(['point_open_preview']);
  });

  it('state and arm call their commands', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(emptyState).mockResolvedValueOnce(undefined);
    await expect(pointApi.state()).resolves.toEqual(emptyState);
    await pointApi.arm();
    expect(invoke).toHaveBeenNthCalledWith(1, 'point_state', undefined);
    expect(invoke).toHaveBeenNthCalledWith(2, 'point_arm', undefined);
  });

  it('removePick sends the pick id as pickId', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(emptyState);
    await expect(pointApi.removePick('pick-7')).resolves.toEqual(emptyState);
    expect(invoke).toHaveBeenCalledWith('point_remove_pick', { pickId: 'pick-7' });
  });

  it('compose returns the text the backend built', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('Make it green.');
    await expect(pointApi.compose('pane-a', 'Make it green.')).resolves.toBe('Make it green.');
    expect(invoke).toHaveBeenCalledWith('point_compose', { targetPaneId: 'pane-a', request: 'Make it green.' });
  });

  it('send returns the outbox message from point_send', async () => {
    const msg = { id: 'm1', text: 'hi', createdAt: 1, state: 'queued', waitingFor: 'The agent is busy.', error: null };
    vi.mocked(invoke).mockResolvedValueOnce(msg);
    await expect(pointApi.send('pane-a', 'hi')).resolves.toEqual(msg);
    expect(invoke).toHaveBeenCalledWith('point_send', { targetPaneId: 'pane-a', request: 'hi' });
  });

  it('editInChat, discard, hide and openBlockedLink call their commands', async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    await pointApi.editInChat('pane-a', 'hi');
    await pointApi.discard();
    await pointApi.hide();
    await pointApi.openBlockedLink();
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ['point_edit_in_chat', { targetPaneId: 'pane-a', request: 'hi' }],
      ['point_discard', undefined],
      ['point_box_hide', undefined],
      ['point_open_blocked_link', undefined],
    ]);
  });

  it('takeDraft returns the draft or null', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('Fix the button.').mockResolvedValueOnce(null);
    await expect(pointApi.takeDraft('pane-a')).resolves.toBe('Fix the button.');
    await expect(pointApi.takeDraft('pane-a')).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith('point_take_draft', { paneId: 'pane-a' });
  });

  it('onState passes each point://state payload to the callback', () => {
    let handler: ((e: { payload: PointState }) => void) | undefined;
    const unlisten = vi.fn();
    vi.mocked(listen).mockImplementation((_name, cb) => {
      handler = cb as (e: { payload: PointState }) => void;
      return Promise.resolve(unlisten);
    });
    const seen: PointState[] = [];
    const off = pointApi.onState((s) => seen.push(s));
    expect(listen).toHaveBeenCalledWith('point://state', expect.any(Function));
    handler?.({ payload: emptyState });
    expect(seen).toEqual([emptyState]);
    off();
  });

  it('onDraft passes the pane id from point://draft to the callback', () => {
    let handler: ((e: { payload: { paneId: string } }) => void) | undefined;
    vi.mocked(listen).mockImplementation((_name, cb) => {
      handler = cb as (e: { payload: { paneId: string } }) => void;
      return Promise.resolve(vi.fn());
    });
    const seen: string[] = [];
    pointApi.onDraft((paneId) => seen.push(paneId));
    expect(listen).toHaveBeenCalledWith('point://draft', expect.any(Function));
    handler?.({ payload: { paneId: 'pane-b' } });
    expect(seen).toEqual(['pane-b']);
  });

  it('unsubscribes the real listener when the caller turns it off', async () => {
    const unlisten = vi.fn();
    vi.mocked(listen).mockResolvedValue(unlisten);
    const off = pointApi.onState(() => {});
    await Promise.resolve();
    await Promise.resolve();
    off();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('unsubscribes at once when the caller turns it off before listen resolves', async () => {
    const unlisten = vi.fn();
    let resolveListen: (u: () => void) => void = () => {};
    vi.mocked(listen).mockReturnValue(new Promise((r) => (resolveListen = r)));
    const off = pointApi.onDraft(() => {});
    off();
    resolveListen(unlisten);
    await new Promise((r) => setTimeout(r, 0));
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
