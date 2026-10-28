// @vitest-environment jsdom
// The Point and Fix draft hook: the Chat box takes the message the Point box put there ("Edit in Chat"), once per
// draft, follows the `point://draft` event for its pane only, and keeps a late draft for the pane instead of losing it.
import { createElement, act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const handlers = new Map<string, (e: { payload: unknown }) => void>();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (event: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(event, cb);
    return () => {
      handlers.delete(event);
    };
  },
}));

import { type ComposerDraft, readDraft, useComposerDraft, writeDraft } from './composerDraft';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
/** The last value the probe rendered. */
let latest: ComposerDraft | null = null;
/** What the backend still holds for each pane (it gives a draft once). */
let pending: Record<string, string | null> = {};

function Probe({ paneId }: { paneId: string | null }) {
  latest = useComposerDraft(paneId);
  return null;
}

const show = (paneId: string | null) => act(() => root.render(createElement(Probe, { paneId })));
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const emit = (paneId: string) => act(() => handlers.get('point://draft')?.({ payload: { paneId } }));

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  localStorage.clear();
  invoke.mockReset();
  handlers.clear();
  pending = {};
  invoke.mockImplementation(async (cmd: string, args: { paneId: string }) => {
    if (cmd !== 'point_take_draft') return undefined;
    const text = pending[args.paneId] ?? null;
    pending[args.paneId] = null;
    return text;
  });
  latest = null;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe('useComposerDraft', () => {
  it('takes the pane draft when the box mounts, and only once', async () => {
    pending['pane-a'] = 'Make the heading bigger';
    show('pane-a');
    await flush();
    expect(invoke).toHaveBeenCalledWith('point_take_draft', { paneId: 'pane-a' });
    expect(latest?.text).toBe('Make the heading bigger');
    expect(latest?.paneId).toBe('pane-a');

    // A second visit finds nothing: the backend gave the draft once.
    show(null);
    show('pane-a');
    await flush();
    expect(latest).toBeNull();
  });

  it('takes again when point://draft names this pane, and ignores other panes', async () => {
    show('pane-a');
    await flush();
    expect(latest).toBeNull();

    pending['pane-b'] = 'for another pane';
    emit('pane-b');
    await flush();
    expect(latest).toBeNull();

    pending['pane-a'] = 'for this pane';
    emit('pane-a');
    await flush();
    expect(latest?.text).toBe('for this pane');
  });

  it('each draft gets a new nonce, so the box can tell it is new', async () => {
    show('pane-a');
    await flush();
    pending['pane-a'] = 'same words';
    emit('pane-a');
    await flush();
    const first = latest?.nonce;
    pending['pane-a'] = 'same words';
    emit('pane-a');
    await flush();
    expect(first).toBeDefined();
    expect(latest?.nonce).not.toBe(first);
  });

  it('a draft is never returned for another pane', async () => {
    pending['pane-a'] = 'for a';
    show('pane-a');
    await flush();
    expect(latest?.text).toBe('for a');
    show('pane-b');
    expect(latest).toBeNull();
    await flush();
    expect(latest).toBeNull();
  });

  it('does nothing without a pane or outside the desktop shell', async () => {
    show(null);
    await flush();
    expect(invoke).not.toHaveBeenCalled();

    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    show('pane-a');
    await flush();
    expect(invoke).not.toHaveBeenCalled();
    expect(latest).toBeNull();
  });

  it('a failed take means no draft, and the box keeps working', async () => {
    invoke.mockRejectedValueOnce(new Error('backend down'));
    show('pane-a');
    await flush();
    expect(latest).toBeNull();
  });

  it('a draft that comes back after the box is gone is kept as the pane draft, not lost', async () => {
    let answer: (text: string) => void = () => {};
    invoke.mockImplementationOnce(() => new Promise<string>((resolve) => (answer = resolve)));
    show('pane-a');
    show(null);
    answer('sent after the box left');
    await flush();
    expect(readDraft('pane-a')).toBe('sent after the box left');
    expect(latest).toBeNull();
  });

  it('a late draft is added after what the pane already kept', async () => {
    writeDraft('pane-a', 'half written');
    let answer: (text: string) => void = () => {};
    invoke.mockImplementationOnce(() => new Promise<string>((resolve) => (answer = resolve)));
    show('pane-a');
    show(null);
    answer('the point text');
    await flush();
    expect(readDraft('pane-a')).toBe('half written\n\nthe point text');
  });
});
