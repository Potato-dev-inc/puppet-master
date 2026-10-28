// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveRoomApi, LiveRoomConfig, PreviewView, StartRequest } from '../../../lib/live-room';
import { GoLiveSheet, orderPanes, type SharePane } from './GoLiveSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// The segment controls measure themselves with a ResizeObserver, which jsdom does not have.
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const LINK = 'https://relay.example.test/r/#aaaaaaaaaaaaaaaaaaaaaaaaaa.c2VjcmV0LXRva2Vu.Y29udGVudC1rZXk';
const ENDS = new Date(2030, 0, 1, 16, 40).getTime();

const PANES: SharePane[] = [
  { id: 'a', label: 'Claude · shop' },
  { id: 'h1', label: 'Helper · tests', parentId: 'a' },
  { id: 'c', label: 'Companion', blocked: 'companion' },
  { id: 's', label: 'Shell 1', blocked: 'no-chat' },
  { id: 'b', label: 'Codex · api' },
];

const PREVIEW: PreviewView = {
  panes: [{ key: 'p1', label: 'Claude · shop', title: 'shop', state: 'working', line: null }],
  items: [{ t: 'user', id: 1, pane: 'p1', text: 'fix the cart' }],
  hidden: 0,
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
});

function fakeApi(config: Partial<LiveRoomConfig> = {}) {
  return {
    configGet: vi.fn(async () => ({ relay: 'https://relay.example.test', minutes: 60, maxViewers: 10, ...config })),
    preview: vi.fn(async (_ids: string[]) => PREVIEW),
    start: vi.fn(async (_req: StartRequest) => ({ endsAtMs: ENDS })),
    copyLink: vi.fn(async () => LINK),
  };
}

async function mount(api: ReturnType<typeof fakeApi> | LiveRoomApi, extra: Partial<Parameters<typeof GoLiveSheet>[0]> = {}) {
  const onClose = vi.fn();
  await act(async () => {
    root.render(<GoLiveSheet panes={PANES} currentPaneId="a" api={api as LiveRoomApi} onClose={onClose} {...extra} />);
  });
  await settle();
  return { onClose };
}

async function settle() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

const rows = () => Array.from(host.querySelectorAll<HTMLLIElement>('.ck-live-pane'));
const boxes = () => Array.from(host.querySelectorAll<HTMLInputElement>('.ck-live-pane input[type="checkbox"]'));
const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === text) as HTMLButtonElement | undefined;
const click = async (el: HTMLElement | undefined) => {
  await act(async () => {
    el?.click();
  });
  await settle();
};
const setTitle = async (value: string) => {
  const input = host.querySelector('.ck-live-input') as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set?.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
};

describe('orderPanes', () => {
  it('puts helpers under their pane and keeps every pane', () => {
    const order = orderPanes(PANES).map((r) => [r.pane.id, r.depth]);
    expect(order).toEqual([
      ['a', 0],
      ['h1', 1],
      ['c', 0],
      ['s', 0],
      ['b', 0],
    ]);
  });
});

describe('Go live sheet', () => {
  it('lists the panes, with the current pane checked', async () => {
    await mount(fakeApi());
    expect(rows().map((r) => r.textContent)).toEqual(
      expect.arrayContaining([expect.stringContaining('Claude · shop'), expect.stringContaining('Helper · tests')]),
    );
    expect(boxes()).toHaveLength(5);
    expect(boxes()[0].checked).toBe(true);
    expect(boxes()[1].checked).toBe(false);
  });

  it('greys companion and shell panes with the reason and does not let them be picked', async () => {
    await mount(fakeApi());
    const blocked = rows().filter((r) => r.classList.contains('is-blocked'));
    expect(blocked).toHaveLength(2);
    expect(blocked[0].textContent).toContain('Companion chats stay private');
    expect(blocked[1].textContent).toContain('No chat for this pane');
    expect(blocked.every((r) => r.querySelector('input')?.disabled)).toBe(true);
  });

  it('the helpers button checks a pane\'s helpers', async () => {
    const api = fakeApi();
    await mount(api);
    await click(button('and its 1 helper'));
    expect(boxes()[1].checked).toBe(true);
    await click(button('Go live'));
    expect(api.start.mock.calls[0][0].paneIds).toEqual(['a', 'h1']);
  });

  it('Go live sends the request shape with the defaults from settings', async () => {
    const api = fakeApi({ minutes: 120, maxViewers: 25 });
    await mount(api);
    await click(button('Go live'));
    expect(api.start).toHaveBeenCalledWith({ paneIds: ['a'], title: 'Live Room', minutes: 120, maxViewers: 25 });
  });

  it('sends the title the person typed', async () => {
    const api = fakeApi();
    await mount(api);
    await setTitle('Cart fix');
    await click(button('Go live'));
    expect(api.start.mock.calls[0][0].title).toBe('Cart fix');
  });

  it('after start shows the masked link and the end time, never the full link', async () => {
    const api = fakeApi();
    await mount(api);
    await click(button('Go live'));
    const text = host.textContent ?? '';
    expect(text).toContain('https://relay.example.test/r/#••••');
    expect(text).not.toContain('c2VjcmV0');
    expect(text).toContain('it ends at 16:40.');
    expect(api.copyLink).toHaveBeenCalled();
  });

  it('shows what viewers would see, from the preview of the picked panes', async () => {
    const api = fakeApi();
    await mount(api);
    expect(api.preview).toHaveBeenCalledWith(['a']);
    expect(host.querySelector('.ck-live-preview')?.textContent).toContain('fix the cart');
  });

  it('with no relay address it says so and links to settings; Go live is off', async () => {
    const onOpenSettings = vi.fn();
    const api = fakeApi({ relay: '' });
    await mount(api, { onOpenSettings });
    expect(host.textContent).toContain('Live Room needs a relay address. Set it in Settings › General › Live Room.');
    expect(button('Go live')?.disabled).toBe(true);
    await click(button('Open Live Room settings'));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it('a team relay counts as a relay', async () => {
    await mount(fakeApi({ relay: '' }), { teamRelay: 'https://team.example.test' });
    expect(host.textContent).not.toContain('Live Room needs a relay address');
    expect(button('Go live')?.disabled).toBe(false);
  });

  it('Go live is off when no pane is picked', async () => {
    await mount(fakeApi());
    await click(boxes()[0]);
    expect(button('Go live')?.disabled).toBe(true);
  });

  it('shows the app message when the start is refused', async () => {
    const api = fakeApi();
    api.start.mockRejectedValue(new Error('Use https:// for a relay on another computer.'));
    await mount(api);
    await click(button('Go live'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Use https:// for a relay on another computer.');
  });
});
