// @vitest-environment jsdom
// The Chat tab and "Make clip": the button on a finished turn and the menu item both open the clip sheet for
// the right turn, and closing the sheet removes it. A chat that is not available offers neither.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn(async () => () => {});
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => (listen as (...x: unknown[]) => unknown)(...a) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import type { RenderClipResult } from '../../lib/clipEncode';
import type { ClipPlan } from '../../lib/clipTypes';
import type { ChatItem } from '../../lib/sessionChat';
import type { ClipApi } from './ClipSheet';
import { SessionChatPanel } from './SessionChatPanel';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const ITEMS: ChatItem[] = [
  { kind: 'user', id: 'u1', at: null, text: 'First' },
  { kind: 'turnEnd', id: 'e1', at: null, outcome: 'completed', durationMs: 1_000 },
  { kind: 'user', id: 'u2', at: null, text: 'Second' },
  { kind: 'turnEnd', id: 'e2', at: null, outcome: 'completed', durationMs: 2_000 },
];

const PLAN: ClipPlan = {
  token: 'tok',
  agent: 'Test Agent',
  request: 'Second',
  turnMs: 2_000,
  outcome: 'completed',
  replay: { kind: 'activity', lengthMs: 4_000 },
  steps: [],
  result: { files: null, checks: null, sentence: 'Done.' },
  notes: [],
  hidden: 0,
};

let host: HTMLDivElement;
let root: Root;
let clipApi: ClipApi;

const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });

function fakeClipApi(): ClipApi {
  return {
    plan: vi.fn(async () => PLAN),
    render: vi.fn(async (): Promise<RenderClipResult> => ({ blob: new Blob([new Uint8Array(8)]), ext: 'mp4', guardHits: 0, usedFallback: false })),
    stage: vi.fn(async () => 8),
    save: vi.fn(async (_t: string, p: string) => p),
    pickPath: vi.fn(async () => null),
  };
}

function setChat(available: boolean) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'session_chat_open') {
      return available ? { available: true, reason: null, items: ITEMS, source: 'file' } : { available: false, reason: 'No chat here.', items: [], source: null };
    }
    if (cmd === 'pane_messages_list') return [];
    return undefined;
  });
}

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  URL.createObjectURL = vi.fn(() => 'blob:x');
  URL.revokeObjectURL = vi.fn();
  invoke.mockReset();
  clipApi = fakeClipApi();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

async function mount() {
  act(() => root.render(<SessionChatPanel paneId="pane-7" clipApi={clipApi} />));
  await flush();
  await flush();
}

const clipButtons = () => [...host.querySelectorAll<HTMLButtonElement>('button.ck-chat-clip-btn')];

describe('SessionChatPanel and Make clip', () => {
  it('the button on a turn opens the sheet for that turn', async () => {
    setChat(true);
    await mount();
    expect(clipButtons()).toHaveLength(2);
    await act(async () => clipButtons()[0].click());
    await flush();
    expect(clipApi.plan).toHaveBeenCalledWith('pane-7', 'u1');
    expect(document.body.textContent).toContain('Make a clip');
  });

  it('the menu item opens the sheet for the last finished turn, and Close removes the sheet', async () => {
    setChat(true);
    await mount();
    act(() => (host.querySelector('[aria-label="Chat menu"]') as HTMLButtonElement).click());
    const item = [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((n) => n.textContent === 'Make clip of last turn');
    expect(item).toBeDefined();
    await act(async () => item!.click());
    await flush();
    expect(clipApi.plan).toHaveBeenCalledWith('pane-7', 'u2');
    const close = [...document.querySelectorAll('button')].find((b) => b.textContent === 'Close') as HTMLButtonElement;
    act(() => close.click());
    expect(document.body.textContent).not.toContain('Make a clip');
  });

  it('a pane without a chat offers no clip', async () => {
    setChat(false);
    await mount();
    expect(clipButtons()).toHaveLength(0);
    expect(host.querySelector('[aria-label="Chat menu"]')).toBeNull();
  });
});
