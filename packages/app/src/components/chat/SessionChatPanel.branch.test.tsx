// @vitest-environment jsdom
// The Chat tab and "Branch": the button on a finished turn opens the branch sheet for that turn's message, a chat
// with no message box (the terminal pane's chat) offers no Branch, and both banners sit under the chat's
// banner slot: the Keep going line and the Branch line.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn(async () => () => {});
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => (listen as (...x: unknown[]) => unknown)(...a) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import type { BranchPreview, BranchView } from '../../lib/branch-from';
import type { HandoverView } from '../../lib/keep-going';
import type { ChatItem } from '../../lib/sessionChat';
import { SessionChatPanel } from './SessionChatPanel';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// Fake labels only.
const ITEMS: ChatItem[] = [
  { kind: 'user', id: 'u1', at: null, text: 'First' },
  { kind: 'assistant', id: 'a1', at: null, text: 'One.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: 'e1', at: null, outcome: 'completed', durationMs: 1_000 },
  { kind: 'user', id: 'u2', at: null, text: 'Second' },
  { kind: 'assistant', id: 'a2', at: null, text: 'Two.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: 'e2', at: null, outcome: 'completed', durationMs: 2_000 },
];

const PREVIEW: BranchPreview = {
  fromLabel: 'Fake A',
  point: { checkpointId: 'cp-1', label: 'after turn 2', atMs: new Date(2026, 9, 28, 14, 2).getTime(), files: 0 },
  canBranch: true,
  whyNot: null,
  offers: [],
  prefill: null,
  turns: 2,
  openBranches: 0,
  brief: null,
};

const NEW_BRANCH: BranchView = {
  id: 'bf-0123456789ab',
  fromLabel: 'Fake A',
  toLabel: 'Fake B',
  fromPaneId: 'pane-7',
  toPaneId: 'pane-9',
  atMs: new Date(2026, 9, 28, 14, 2).getTime(),
  pointLabel: 'after turn 2 · 14:02',
  state: 'running',
  stateText: 'Running in a separate copy',
  turns: 2,
  briefBytes: 100,
  branchName: 'pm/bf-0123456789ab',
  closed: null,
  failedFiles: 0,
};

const HANDOVER: HandoverView = {
  id: 'kg-0123456789ab',
  fromLabel: 'Fake A',
  toLabel: 'Fake C',
  fromPaneId: 'pane-x',
  toPaneId: 'pane-7',
  atMs: new Date(2026, 9, 28, 17, 2).getTime(),
  state: 'running',
  stateText: 'Continued from Fake A',
  turns: 14,
  briefBytes: 100,
  backAtMs: null,
  reason: 'usage_limit',
};

let host: HTMLDivElement;
let root: Root;
let branchLineage: { from: BranchView | null; children: BranchView[] };
let keepGoingLineage: { from: HandoverView | null; to: HandoverView | null };

const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockReset();
  branchLineage = { from: null, children: [] };
  keepGoingLineage = { from: null, to: null };
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'session_chat_open') return { available: true, reason: null, items: ITEMS, source: 'file' };
    if (cmd === 'pane_messages_list') return [];
    if (cmd === 'branch_lineage') return branchLineage;
    if (cmd === 'keep_going_lineage') return keepGoingLineage;
    if (cmd === 'branch_preview') return PREVIEW;
    return undefined;
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  document.body.querySelectorAll('[role="dialog"]').forEach((n) => n.remove());
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

async function mount(reply: boolean) {
  act(() => {
    root.render(<SessionChatPanel paneId="pane-7" reply={reply ? { blockedReason: null } : undefined} />);
  });
  await flush();
  await flush();
}

const branchButtons = () => [...host.querySelectorAll<HTMLButtonElement>('button.ck-chat-branch-btn')];

describe('SessionChatPanel and Branch', () => {
  it('shows Branch on each finished turn of a chat with a message box', async () => {
    await mount(true);
    expect(branchButtons()).toHaveLength(2);
    expect(branchButtons().every((b) => b.textContent === 'Branch')).toBe(true);
  });

  it('shows no Branch in a chat with no message box (read-only)', async () => {
    await mount(false);
    expect(branchButtons()).toHaveLength(0);
  });

  it('opens the branch sheet for the message that starts the turn, placed after the turn', async () => {
    await mount(true);
    act(() => branchButtons()[1].click());
    await flush();
    expect(invoke).toHaveBeenCalledWith('branch_preview', {
      paneId: 'pane-7',
      from: { kind: 'message', user_item_id: 'u2', when: 'after' },
      target: null,
      message: null,
    });
    expect(document.body.textContent).toContain('Branch from here');
  });

  it('shows the Keep going line and the Branch line under the chat banner slot', async () => {
    branchLineage = { from: NEW_BRANCH, children: [] };
    keepGoingLineage = { from: null, to: HANDOVER };
    await mount(true);
    expect(host.querySelector('[data-keep-going]')).not.toBeNull();
    expect(host.querySelector('[data-branch-banner="new"]')?.textContent).toContain('Branched from Fake A');
  });
});
