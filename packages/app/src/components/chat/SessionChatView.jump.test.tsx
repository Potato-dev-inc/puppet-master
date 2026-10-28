// @vitest-environment jsdom
// A search result opens a chat at one message: the chat scrolls to its turn and marks it for a moment.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearChatJump, requestChatJump, useChatJump } from '../../lib/chatJump';
import { groupTurns, type ChatItem } from '../../lib/sessionChat';
import { SessionChatView } from './SessionChatView';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let scrolled: HTMLElement[];

beforeEach(() => {
  vi.useFakeTimers();
  scrolled = [];
  Element.prototype.scrollIntoView = function (this: HTMLElement) {
    scrolled.push(this);
  };
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const ITEMS: ChatItem[] = [
  { kind: 'user', id: '10:0', at: null, text: 'List the files' },
  { kind: 'assistant', id: '50:0', at: null, text: 'Two entries.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: '60:0', at: null, outcome: 'completed', durationMs: 1000 },
  { kind: 'user', id: '70:0', at: null, text: 'Now the tests' },
  { kind: 'assistant', id: '90:0', at: null, text: 'All pass.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: '99:0', outcome: 'completed', at: null, durationMs: 1000 },
];

function Chat({ items }: { items: ChatItem[] }) {
  const jump = useChatJump('p1');
  return <SessionChatView state="ready" reason={null} turns={groupTurns(items).turns} model={null} cliVersion={null} paneId="p1" jump={jump} />;
}

describe('SessionChatView jump', () => {
  it('scrolls to the turn that holds the message, marks it, then lets go of the jump', () => {
    act(() => root.render(<Chat items={ITEMS} />));
    act(() => requestChatJump('p1', '90:0'));
    const second = host.querySelectorAll<HTMLElement>('[data-turn-id]')[1];
    expect(scrolled).toEqual([second]);
    expect(second.classList.contains('is-found')).toBe(true);
    act(() => {
      vi.advanceTimersByTime(2100);
    });
    expect(second.classList.contains('is-found')).toBe(false);
    // The jump was handled once: a later render does not scroll again.
    act(() => root.render(<Chat items={[...ITEMS]} />));
    expect(scrolled).toHaveLength(1);
  });

  it('waits for a message that is not loaded yet, and scrolls when it comes', () => {
    act(() => root.render(<Chat items={ITEMS.slice(0, 3)} />));
    act(() => requestChatJump('p1', '90:0'));
    expect(scrolled).toHaveLength(0);
    act(() => root.render(<Chat items={ITEMS} />));
    expect(scrolled).toHaveLength(1);
  });

  it('drops a jump to a message the chat never shows', () => {
    act(() => root.render(<Chat items={ITEMS} />));
    act(() => requestChatJump('p1', '123:0'));
    expect(scrolled).toHaveLength(0);
    act(() => {
      vi.advanceTimersByTime(10_100);
    });
    // Gone: a chat that shows up later is not scrolled by an old request.
    act(() => root.render(<Chat items={[...ITEMS, { kind: 'user', id: '123:0', at: null, text: 'late' }]} />));
    expect(scrolled).toHaveLength(0);
    // (nothing left to clear)
    clearChatJump({ paneId: 'p1', itemId: '123:0', nonce: -1 });
  });
});
