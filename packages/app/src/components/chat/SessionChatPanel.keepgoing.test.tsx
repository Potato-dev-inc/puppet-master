// @vitest-environment jsdom
// The Keep going line in the Chat tab: it sits above the conversation, reads per side, Open focuses the other
// pane through the callback the cockpit gives, and it refreshes when `keep-going://changed` names this pane.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const handlers = new Map<string, (e: { payload: unknown }) => void>();
const listen = vi.fn(async (event: string, cb: (e: { payload: unknown }) => void) => {
  handlers.set(event, cb);
  return () => handlers.delete(event);
});
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => listen(...(a as Parameters<typeof listen>)) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import type { HandoverView, KeepGoingLineage } from '../../lib/keep-going';
import { SessionChatPanel } from './SessionChatPanel';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Fake labels only.
const handover: HandoverView = {
  id: 'kg-0123456789ab',
  fromLabel: 'Fake A',
  toLabel: 'Fake B',
  fromPaneId: 'pane-a',
  toPaneId: 'pane-b',
  atMs: new Date(2026, 9, 28, 17, 2).getTime(),
  state: 'running',
  stateText: 'Continued on Fake B',
  turns: 14,
  briefBytes: 100,
  backAtMs: null,
  reason: 'usage_limit',
};

let host: HTMLDivElement;
let root: Root;
let lineage: Record<string, KeepGoingLineage>;

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const row = () => host.querySelector<HTMLElement>('[data-keep-going]');

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockReset();
  listen.mockClear();
  handlers.clear();
  lineage = { 'pane-a': { from: null, to: handover }, 'pane-b': { from: handover, to: null } };
  invoke.mockImplementation(async (cmd: string, args: Record<string, unknown>) => {
    if (cmd === 'session_chat_open') return { available: true, reason: null, items: [], source: 'file' };
    if (cmd === 'pane_messages_list') return [];
    if (cmd === 'keep_going_lineage') return lineage[String(args.paneId)] ?? { from: null, to: null };
    return undefined;
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

async function mount(paneId: string, onOpenPane?: (id: string) => void) {
  await act(async () => {
    root.render(<SessionChatPanel paneId={paneId} onOpenPane={onOpenPane} />);
  });
  await flush();
}

describe('Keep going line in the chat panel', () => {
  it('shows nothing for a pane that was never handed over', async () => {
    await mount('pane-x');
    expect(row()).toBeNull();
    expect(invoke).toHaveBeenCalledWith('keep_going_lineage', { paneId: 'pane-x' });
  });

  it('sits above the conversation and says "Continued on" on the old pane, with Open calling the pane-focus callback', async () => {
    const open = vi.fn();
    await mount('pane-a', open);
    expect(row()!.textContent).toContain('Continued on Fake B');
    const body = host.querySelector('.ck-chat-body')!;
    // The line comes before the scrolling conversation in the document.
    expect(row()!.compareDocumentPosition(body) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const openBtn = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Open')!;
    await act(async () => openBtn.click());
    expect(open).toHaveBeenCalledWith('pane-b');
  });

  it('says "Continued from" on the new pane, not "Continued on"', async () => {
    await mount('pane-b');
    expect(row()!.textContent).toContain('Continued from Fake A');
    expect(row()!.textContent).not.toContain('Continued on');
  });

  it('refreshes when the changed event names this pane', async () => {
    lineage = {};
    await mount('pane-a');
    expect(row()).toBeNull();
    lineage = { 'pane-a': { from: null, to: handover } };
    await act(async () => handlers.get('keep-going://changed')?.({ payload: { paneIds: ['pane-a'] } }));
    await flush();
    expect(row()!.textContent).toContain('Continued on Fake B');
  });
});
