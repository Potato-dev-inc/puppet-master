// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { clearChatJump, requestChatJump, turnWithItem, useChatJump, type ChatJump } from './chatJump';
import { groupTurns, type ChatItem } from './sessionChatModel';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ITEMS: ChatItem[] = [
  { kind: 'user', id: '10:0', at: null, text: 'List the files' },
  { kind: 'toolCall', id: '30:0', at: null, callId: 'c1', name: 'Bash', summary: 'ls' },
  { kind: 'assistant', id: '50:0', at: null, text: 'Two entries.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: '60:0', at: null, outcome: 'completed', durationMs: 1000 },
  { kind: 'user', id: '70:0', at: null, text: 'Thanks' },
];

describe('turnWithItem', () => {
  it('finds the turn of a message, an answer or a tool call', () => {
    const turns = groupTurns(ITEMS).turns;
    expect(turnWithItem(turns, '10:0')).toBe(turns[0]);
    expect(turnWithItem(turns, '30:0')).toBe(turns[0]);
    expect(turnWithItem(turns, '50:0')).toBe(turns[0]);
    expect(turnWithItem(turns, '70:0')).toBe(turns[1]);
    expect(turnWithItem(turns, '99:0')).toBeNull();
  });
});

describe('useChatJump', () => {
  let host: HTMLDivElement;
  let root: Root;
  const seen: Record<string, ChatJump | null> = {};

  function Probe({ paneId }: { paneId: string }) {
    seen[paneId] = useChatJump(paneId);
    return null;
  }

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
    act(() => {
      root.render(
        <>
          <Probe paneId="p1" />
          <Probe paneId="p2" />
        </>,
      );
    });
  });
  afterEach(() => {
    if (seen.p1) clearChatJump(seen.p1);
    if (seen.p2) clearChatJump(seen.p2);
    act(() => root.unmount());
    host.remove();
  });

  it('offers a jump only to the pane it is for, until it is cleared', () => {
    act(() => requestChatJump('p2', '50:0'));
    expect(seen.p1).toBeNull();
    expect(seen.p2).toMatchObject({ paneId: 'p2', itemId: '50:0' });
    act(() => clearChatJump(seen.p2!));
    expect(seen.p2).toBeNull();
  });

  it('a newer jump replaces an older one, and clearing the older one does not remove it', () => {
    act(() => requestChatJump('p1', '10:0'));
    const older = seen.p1!;
    act(() => requestChatJump('p1', '50:0'));
    expect(seen.p1?.itemId).toBe('50:0');
    expect(seen.p1?.nonce).not.toBe(older.nonce);
    act(() => clearChatJump(older));
    expect(seen.p1?.itemId).toBe('50:0');
  });
});
