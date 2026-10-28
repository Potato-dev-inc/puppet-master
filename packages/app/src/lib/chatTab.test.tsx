// @vitest-environment jsdom
// The Chat tab request: delivered to its own pane only, cleared once, and an old clear never drops a newer request.
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { clearChatTabRequest, requestChatTab, useChatTabRequest, type ChatTabRequest } from './chatTab';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let seen: ChatTabRequest | null;

function Probe({ paneId }: { paneId: string }) {
  seen = useChatTabRequest(paneId);
  return null;
}

beforeEach(() => {
  seen = null;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  if (seen) act(() => clearChatTabRequest(seen!));
  act(() => root.unmount());
  host.remove();
});

describe('chatTab', () => {
  it('delivers a request to its own pane only', () => {
    act(() => root.render(createElement(Probe, { paneId: 'p1' })));
    expect(seen).toBeNull();
    act(() => requestChatTab('p2'));
    expect(seen).toBeNull();
    act(() => requestChatTab('p1'));
    expect(seen?.paneId).toBe('p1');
  });

  it('clears once, and an old clear does not drop a newer request', () => {
    act(() => root.render(createElement(Probe, { paneId: 'p1' })));
    act(() => requestChatTab('p1'));
    const first = seen!;
    act(() => requestChatTab('p1'));
    act(() => clearChatTabRequest(first));
    expect(seen).not.toBeNull();
    act(() => clearChatTabRequest(seen!));
    expect(seen).toBeNull();
  });
});
