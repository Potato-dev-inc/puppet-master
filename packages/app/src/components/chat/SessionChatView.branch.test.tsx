// @vitest-environment jsdom
// "Branch" on a turn's end line: only on a finished turn that a message started, only when the view is given
// `canBranch` and `onBranch`, never in a read-only chat. It names the message that starts the turn. The newest
// finished turn keeps it in view; the others show it on hover. It does not touch the Make clip buttons.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { groupTurns, type ChatItem } from '../../lib/sessionChat';
import { SessionChatView, type SessionChatViewProps } from './SessionChatView';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const FINISHED_1: ChatItem[] = [
  { kind: 'user', id: 'u1', at: null, text: 'First' },
  { kind: 'assistant', id: 'a1', at: null, text: 'One.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: 'e1', at: null, outcome: 'completed', durationMs: 5_000 },
];
const FINISHED_2: ChatItem[] = [
  { kind: 'user', id: 'u2', at: null, text: 'Second' },
  { kind: 'assistant', id: 'a2', at: null, text: 'Two.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: 'e2', at: null, outcome: 'completed', durationMs: 6_000 },
];
const RUNNING: ChatItem[] = [
  { kind: 'user', id: 'u3', at: null, text: 'Third' },
  { kind: 'assistant', id: 'a3', at: null, text: 'Working.', model: null, finalAnswer: false },
];
const NO_USER: ChatItem[] = [
  { kind: 'assistant', id: 'a0', at: null, text: 'Hello.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: 'e0', at: null, outcome: 'completed', durationMs: 1_000 },
];

function render(items: ChatItem[], props: Partial<SessionChatViewProps> = {}) {
  act(() => {
    root.render(<SessionChatView state="ready" reason={null} turns={groupTurns(items).turns} model={null} cliVersion={null} {...props} />);
  });
}

const branchButtons = () => [...host.querySelectorAll<HTMLButtonElement>('button.ck-chat-branch-btn')];
const clipButtons = () => [...host.querySelectorAll<HTMLButtonElement>('button.ck-chat-clip-btn')];

describe('Branch on the turn end line', () => {
  it('shows on each finished turn, and not on a running one', () => {
    render([...FINISHED_1, ...FINISHED_2, ...RUNNING], { canBranch: true, onBranch: () => {} });
    expect(branchButtons()).toHaveLength(2);
    expect(branchButtons().every((b) => b.textContent === 'Branch')).toBe(true);
    // The button sits in the end line of its turn.
    expect(branchButtons().every((b) => b.closest('.ck-chat-end'))).toBe(true);
  });

  it('is not there while the only turn runs', () => {
    render(RUNNING, { canBranch: true, onBranch: () => {} });
    expect(branchButtons()).toHaveLength(0);
  });

  it('is not there without canBranch or without a handler', () => {
    render(FINISHED_1);
    expect(branchButtons()).toHaveLength(0);
    render(FINISHED_1, { canBranch: true });
    expect(branchButtons()).toHaveLength(0);
    render(FINISHED_1, { onBranch: () => {} });
    expect(branchButtons()).toHaveLength(0);
  });

  it('is never there in a read-only chat, even when asked for', () => {
    render(FINISHED_1, { canBranch: true, onBranch: () => {}, readOnly: true });
    expect(branchButtons()).toHaveLength(0);
  });

  it('is not there on a turn that no message started', () => {
    render(NO_USER, { canBranch: true, onBranch: () => {} });
    expect(branchButtons()).toHaveLength(0);
  });

  it('passes the id of the message that starts the turn', () => {
    const onBranch = vi.fn();
    render([...FINISHED_1, ...FINISHED_2], { canBranch: true, onBranch });
    act(() => branchButtons()[0].click());
    expect(onBranch).toHaveBeenLastCalledWith('u1');
    act(() => branchButtons()[1].click());
    expect(onBranch).toHaveBeenLastCalledWith('u2');
  });

  it('keeps the newest finished turn in view and shows the others on hover', () => {
    render([...FINISHED_1, ...FINISHED_2, ...RUNNING], { canBranch: true, onBranch: () => {} });
    const [older, newest] = branchButtons();
    expect(older.classList.contains('is-always')).toBe(false);
    expect(newest.classList.contains('is-always')).toBe(true);
  });

  it('sits next to Make clip and leaves the clip buttons as they were', () => {
    render([...FINISHED_1, ...FINISHED_2], { canBranch: true, onBranch: () => {}, canClip: true, onMakeClip: () => {} });
    expect(branchButtons()).toHaveLength(2);
    expect(clipButtons()).toHaveLength(2);
    expect(clipButtons().every((b) => b.textContent === 'Make clip')).toBe(true);
  });
});
