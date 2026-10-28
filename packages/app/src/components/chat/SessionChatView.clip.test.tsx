// @vitest-environment jsdom
// "Make clip" on a turn's end line: only on a finished turn that a message started, only when the desktop Chat tab
// asks for it (the phone does not), and it names the message that starts the turn. The newest one stays in view.
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

function render(items: ChatItem[], props: Partial<SessionChatViewProps> = {}) {
  act(() => {
    root.render(<SessionChatView state="ready" reason={null} turns={groupTurns(items).turns} model={null} cliVersion={null} {...props} />);
  });
}

const clipButtons = () => [...host.querySelectorAll<HTMLButtonElement>('button.ck-chat-clip-btn')];

describe('Make clip on the turn end line', () => {
  it('shows on each finished turn, and not on a running one', () => {
    render([...FINISHED_1, ...FINISHED_2, ...RUNNING], { canClip: true, onMakeClip: () => {} });
    expect(clipButtons()).toHaveLength(2);
    expect(clipButtons().every((b) => b.textContent === 'Make clip')).toBe(true);
    // The button sits in the end line of its turn.
    expect(clipButtons().every((b) => b.closest('.ck-chat-end'))).toBe(true);
  });

  it('is not there while the only turn runs', () => {
    render(RUNNING, { canClip: true, onMakeClip: () => {} });
    expect(clipButtons()).toHaveLength(0);
  });

  it('is not there without canClip or without a handler (the phone)', () => {
    render(FINISHED_1);
    expect(clipButtons()).toHaveLength(0);
    render(FINISHED_1, { canClip: true });
    expect(clipButtons()).toHaveLength(0);
    render(FINISHED_1, { onMakeClip: () => {} });
    expect(clipButtons()).toHaveLength(0);
  });

  it('is not there on a turn that no message started', () => {
    render(
      [
        { kind: 'assistant', id: 'a0', at: null, text: 'Hello.', model: null, finalAnswer: true },
        { kind: 'turnEnd', id: 'e0', at: null, outcome: 'completed', durationMs: 1_000 },
      ],
      { canClip: true, onMakeClip: () => {} },
    );
    expect(clipButtons()).toHaveLength(0);
  });

  it('passes the id of the message that starts the turn', () => {
    const onMakeClip = vi.fn();
    render([...FINISHED_1, ...FINISHED_2], { canClip: true, onMakeClip });
    act(() => clipButtons()[0].click());
    expect(onMakeClip).toHaveBeenLastCalledWith('u1');
    act(() => clipButtons()[1].click());
    expect(onMakeClip).toHaveBeenLastCalledWith('u2');
  });

  it('keeps the newest finished turn in view and shows the others on hover', () => {
    render([...FINISHED_1, ...FINISHED_2, ...RUNNING], { canClip: true, onMakeClip: () => {} });
    const [older, newest] = clipButtons();
    expect(older.classList.contains('is-always')).toBe(false);
    expect(newest.classList.contains('is-always')).toBe(true);
  });

  it('is offered on an interrupted turn too', () => {
    render(
      [
        { kind: 'user', id: 'u9', at: null, text: 'Stop' },
        { kind: 'turnEnd', id: 'e9', at: null, outcome: 'interrupted', durationMs: 2_000 },
      ],
      { canClip: true, onMakeClip: () => {} },
    );
    expect(clipButtons()).toHaveLength(1);
  });
});
