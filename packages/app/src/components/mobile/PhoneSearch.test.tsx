// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { BridgeClient } from '../../lib/bridge';
import type { ChatHit } from '../../lib/chatSearch';
import { PhoneSearch, SEARCH_MAX_CHARS, SEARCH_OFF_TEXT } from './PhoneSearch';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const pane = (id: string) => ({ id, status: 'running', agent_type: 'claude', cwd: 'C:\\work\\shop' }) as unknown as PaneInfo;

const hit = (over: Partial<ChatHit> = {}): ChatHit => ({
  paneId: 'p1',
  session: 's1',
  itemId: 'i1',
  kind: 'assistant',
  agent: 'claude',
  folder: 'shop',
  atMs: Date.now() - 5 * 60_000,
  // "Found the " is 10 units, so "license" is [10, 17).
  snippet: 'Found the license key in checks.rs',
  marks: [[10, 17]],
  ...over,
});

const agentName = (id: string) => (id === 'claude' ? 'Claude' : id);

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

/** The search waits 150 ms after the last key: let it run and the answer land. */
async function settle() {
  await act(async () => {
    await new Promise((done) => setTimeout(done, 220));
  });
}

function mount(bridge: Partial<BridgeClient>, panes: PaneInfo[] = [pane('p1')]) {
  const onOpenChat = vi.fn();
  const onClose = vi.fn();
  act(() => {
    root.render(
      <PhoneSearch bridge={bridge as BridgeClient} panes={panes} agentName={agentName} onOpenChat={onOpenChat} onClose={onClose} />,
    );
  });
  return { onOpenChat, onClose };
}

function typeText(text: string) {
  const input = host.querySelector<HTMLInputElement>('input')!;
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  act(() => {
    setValue.call(input, text);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

const text = () => host.querySelector('.ph-search')?.textContent ?? '';
const hits = () => Array.from(host.querySelectorAll<HTMLButtonElement>('.ph-search-hit'));
const tap = (element: Element) => act(() => (element as HTMLElement).click());

describe('PhoneSearch', () => {
  it('asks the PC with what was typed and at most 30 hits', async () => {
    const searchChats = vi.fn(async () => ({ enabled: true, hits: [] }));
    mount({ searchChats });
    typeText('license');
    await settle();
    expect(searchChats).toHaveBeenCalledTimes(1);
    expect(searchChats).toHaveBeenCalledWith('license', 30);
  });

  it('does not ask anything while the box is empty', async () => {
    const searchChats = vi.fn(async () => ({ enabled: true, hits: [] }));
    mount({ searchChats });
    await settle();
    expect(searchChats).not.toHaveBeenCalled();
    expect(text()).toContain('Type a word');
  });

  it('keeps the search box at 200 characters', () => {
    mount({ searchChats: vi.fn(async () => ({ enabled: true, hits: [] })) });
    expect(host.querySelector('input')!.maxLength).toBe(SEARCH_MAX_CHARS);
    expect(SEARCH_MAX_CHARS).toBe(200);
  });

  it('shows each hit as who said it, the marked words, and agent, folder and time', async () => {
    mount({ searchChats: vi.fn(async () => ({ enabled: true, hits: [hit()] })) });
    typeText('license');
    await settle();
    expect(hits()).toHaveLength(1);
    const row = hits()[0];
    expect(row.querySelector('.ph-search-who')?.textContent).toBe('Agent');
    const mark = row.querySelector('mark.ph-search-mark');
    expect(mark?.textContent).toBe('license');
    expect(row.querySelector('.ph-search-text')?.textContent).toBe('Found the license key in checks.rs');
    expect(row.querySelector('.ph-search-meta')?.textContent).toBe('Claude · shop · 5 min ago');
  });

  it('says search is off, with the words to turn it on, when the PC has it off', async () => {
    mount({ searchChats: vi.fn(async () => ({ enabled: false, hits: [] })) });
    typeText('license');
    await settle();
    expect(hits()).toHaveLength(0);
    expect(text()).toContain(SEARCH_OFF_TEXT);
    expect(SEARCH_OFF_TEXT).toBe('Search is off — turn it on in Settings on your computer');
  });

  it('says no chat matches when nothing matches the words', async () => {
    mount({ searchChats: vi.fn(async () => ({ enabled: true, hits: [] })) });
    typeText('zebra');
    await settle();
    expect(text()).toContain('No chat says “zebra”.');
  });

  it('says it could not search when the PC does not answer', async () => {
    mount({ searchChats: vi.fn(async () => Promise.reject(new Error('bridge down'))) });
    typeText('license');
    await settle();
    expect(text()).toContain('Could not search');
    expect(text()).not.toContain('bridge down');
  });

  it('opens the chat of a hit whose pane is still open, and calls nothing else', async () => {
    const { onOpenChat } = mount({ searchChats: vi.fn(async () => ({ enabled: true, hits: [hit({ paneId: 'p1' })] })) }, [pane('p1')]);
    typeText('license');
    await settle();
    tap(hits()[0]);
    expect(onOpenChat).toHaveBeenCalledWith('p1');
    expect(text()).not.toContain('This chat is closed');
  });

  it('shows only the kept snippet, read-only, for a hit whose pane is closed', async () => {
    const { onOpenChat } = mount({ searchChats: vi.fn(async () => ({ enabled: true, hits: [hit({ paneId: 'gone' })] })) }, [pane('p1')]);
    typeText('license');
    await settle();
    expect(hits()[0].querySelector('.ph-search-closed')?.textContent).toBe('Closed');
    tap(hits()[0]);
    expect(onOpenChat).not.toHaveBeenCalled();
    expect(text()).toContain('This chat is closed, so this is all that was kept.');
    expect(host.querySelector('.ph-search-detail-text mark')?.textContent).toBe('license');
    // Back to the results, and the same hit is still there.
    tap(Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Back to results')!);
    expect(hits()).toHaveLength(1);
  });

  it('closes from the cancel button', () => {
    const { onClose } = mount({ searchChats: vi.fn(async () => ({ enabled: true, hits: [] })) });
    tap(Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Cancel')!);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
