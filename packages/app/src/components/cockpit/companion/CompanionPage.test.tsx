// @vitest-environment jsdom
// The companion's page: header buttons and their commands, the chat for its pane, the sleeping and waking-up
// states, the rail (hide and show), rename and archive.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type CompanionView, type FeedItem } from '../../../lib/companion';
import { CompanionPage, type CompanionPageProps } from './CompanionPage';

// The chat is the Chat tab's panel, tested with the pane; here it only needs to show which pane it was given.
vi.mock('../../chat/SessionChatPanel', async () => {
  const React = await import('react');
  return {
    SessionChatPanel: ({ paneId, emptyNote }: { paneId: string; emptyNote?: React.ReactNode }) =>
      React.createElement('div', { 'data-testid': 'chat' }, paneId, emptyNote),
  };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

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
  vi.restoreAllMocks();
});

const view = (over: Partial<CompanionView> = {}): CompanionView => ({
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folder: 'C:\\work\\home',
  folderName: 'home',
  state: 'idle',
  stateLine: 'Here. Ask me anything.',
  paneId: 'companion-mochi',
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
  ...over,
});

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label);

async function render(companion: CompanionView, over: Partial<CompanionPageProps> = {}, feed: FeedItem[] = []) {
  const transport = fakeCompanionTransport({
    companions: [companion],
    memory: { soul: 'Warm.', user: '', memory: '', pending: [] },
    feed,
  });
  const props: CompanionPageProps = {
    companion,
    transport,
    onShowTerminal: vi.fn(),
    onOpenCard: vi.fn(),
    onNotify: vi.fn(),
    onChanged: vi.fn(),
    onArchived: vi.fn(),
    ...over,
  };
  await act(async () => root.render(<CompanionPage {...props} />));
  await act(async () => {});
  return { transport, props };
}

describe('CompanionPage', () => {
  it('shows the face, the name and the state line, and the chat for its own pane', async () => {
    await render(view());
    expect(host.querySelector('.ck-companion-name')?.textContent).toBe('Mochi');
    expect(host.querySelector('.ck-companion-stateline')?.textContent).toBe('Here. Ask me anything.');
    expect(host.querySelector('[data-testid="chat"]')?.textContent).toBe('companion-mochi');
  });

  it('turns the check-in editor on when the view carries the list, and keeps what it cannot see off', async () => {
    await render(view({ checkins: [{ id: 'a1b2c3d4', label: 'Morning', text: 'Say hi', when: { kind: 'daily', at: { h: 8, m: 0 } }, enabled: true }] }));
    expect(host.textContent).toContain('Morning');
    expect(button('Add a check-in')).toBeTruthy();
  });

  it('offers no editor while the view has no check-in list (a save would replace check-ins it cannot see)', async () => {
    await render(view());
    expect(button('Add a check-in')).toBeUndefined();
  });

  it('says a new session starts after a nap or wake-up, when the companion slept before', async () => {
    await render(view(), {}, [{ id: 'f1', atMs: 1000, kind: 'slept', text: 'Went to sleep.' }]);
    expect(host.querySelector('[data-testid="chat"]')?.textContent).toContain('New session after a nap or wake-up');
  });

  it('shows no new-session line for a companion that has never slept (its first start is only a wake-up)', async () => {
    await render(view(), {}, [{ id: 'f1', atMs: 1000, kind: 'woke', text: 'Woke up.' }]);
    expect(host.querySelector('[data-testid="chat"]')?.textContent).not.toContain('New session');
  });

  it('Sleep sends the sleep command', async () => {
    const { transport, props } = await render(view());
    await act(async () => button('Sleep')!.click());
    expect(transport.calls.some((c) => c.method === 'sleep' && c.args[0] === 'mochi')).toBe(true);
    expect(props.onChanged).toHaveBeenCalled();
  });

  it('a sleeping companion shows no chat, and Wake up sends the wake command', async () => {
    const { transport } = await render(view({ state: 'sleeping', stateLine: 'Sleeping. Wake me when you need me.' }));
    expect(host.querySelector('[data-testid="chat"]')).toBeNull();
    expect(host.textContent).toContain('Sleeping. Wake me when you need me.');
    await act(async () => button('Wake up Mochi')!.click());
    expect(transport.calls.some((c) => c.method === 'wake')).toBe(true);
  });

  it('a companion that is still starting has no chat yet', async () => {
    await render(view({ paneId: null, state: 'starting', stateLine: 'Waking up…' }));
    expect(host.querySelector('[data-testid="chat"]')).toBeNull();
    expect(host.textContent).toContain('Waking up…');
  });

  it('when it needs the person, Open card goes to the Inbox', async () => {
    const { props } = await render(view({ state: 'needs_you', needs: { itemId: 'i1', title: 'Approve a command' }, stateLine: 'Needs you: Approve a command' }));
    await act(async () => button('Open card')!.click());
    expect(props.onOpenCard).toHaveBeenCalledTimes(1);
  });

  it('Show terminal opens the pane in the pane view', async () => {
    const { props } = await render(view());
    await act(async () => button('Show terminal')!.click());
    expect(props.onShowTerminal).toHaveBeenCalledWith('companion-mochi');
  });

  it('the rail shows the memory and the setup, and hides and shows again', async () => {
    await render(view());
    expect(host.querySelector('.ck-companion-rail')?.textContent).toContain('What Mochi knows');
    expect(host.querySelector('.ck-companion-rail')?.textContent).toContain('Claude Code');
    act(() => host.querySelector<HTMLButtonElement>('button[aria-label="Hide details"]')!.click());
    expect(host.querySelector('.ck-companion-rail')).toBeNull();
    act(() => host.querySelector<HTMLButtonElement>('button[aria-label="Show details"]')!.click());
    expect(host.querySelector('.ck-companion-rail')).not.toBeNull();
  });

  it('the name is renamed in place and saved on leaving the field', async () => {
    const { transport } = await render(view());
    act(() => host.querySelector<HTMLButtonElement>('.ck-companion-name')!.click());
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Companion name"]')!;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
    act(() => {
      setter.call(input, 'Pip');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => {
      input.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
    });
    expect(transport.calls.find((c) => c.method === 'update')?.args).toEqual(['mochi', { name: 'Pip' }]);
  });

  it('archive asks first, then archives and leaves the page', async () => {
    const { transport, props } = await render(view());
    act(() => host.querySelector<HTMLButtonElement>('button[aria-label="More"]')!.click());
    act(() => host.querySelector<HTMLButtonElement>('[role="menuitem"]:nth-child(2)')!.click());
    expect(host.textContent).toContain('Archive Mochi?');
    await act(async () => host.querySelector<HTMLButtonElement>('.ck-companion-sheet .ck-btn--primary')!.click());
    expect(transport.calls.some((c) => c.method === 'archive' && c.args[0] === 'mochi')).toBe(true);
    expect(props.onArchived).toHaveBeenCalledTimes(1);
  });
});
