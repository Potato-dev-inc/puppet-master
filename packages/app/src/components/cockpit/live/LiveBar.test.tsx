// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveRoomApi, LiveStatus } from '../../../lib/live-room';
import { LiveBar } from './LiveBar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const LINK = 'https://relay.example.test/r/#aaaaaaaaaaaaaaaaaaaaaaaaaa.c2VjcmV0LXRva2Vu.Y29udGVudC1rZXk';

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

/** A live room that ends in about 41 minutes, with 7 viewers. */
const live = (over: Partial<LiveStatus> = {}): LiveStatus => ({
  live: true,
  paused: false,
  viewers: 7,
  endsAtMs: Date.now() + 41 * 60_000 - 30_000,
  paneIds: ['p1'],
  hidden: 0,
  stopped: null,
  ...over,
});

/** A fake api whose `status()` reads the room as the actions leave it. */
function fakeApi(start: LiveStatus | null) {
  let room = start;
  const api = {
    status: vi.fn(async () => room),
    pause: vi.fn(async () => {
      room = { ...room!, paused: true };
    }),
    resume: vi.fn(async () => {
      room = { ...room!, paused: false };
    }),
    stop: vi.fn(async () => {
      room = { ...room!, live: false, paused: false };
    }),
    copyLink: vi.fn(async () => LINK),
  };
  return api as typeof api & LiveRoomApi;
}

const noSubscribe = vi.fn(async () => () => {});

async function settle() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

async function mount(api: LiveRoomApi, onStopped?: () => void) {
  await act(async () => {
    root.render(<LiveBar api={api} subscribe={noSubscribe} onStopped={onStopped} />);
  });
  await settle();
}

const buttons = () => Array.from(host.querySelectorAll('button'));
const button = (text: string) => buttons().find((b) => b.textContent?.trim() === text) as HTMLButtonElement | undefined;
const click = async (el: HTMLElement | undefined) => {
  await act(async () => {
    el?.click();
  });
  await settle();
};

describe('LiveBar', () => {
  it('shows LIVE, the viewer count and the minutes left', async () => {
    await mount(fakeApi(live()));
    const text = host.textContent ?? '';
    expect(text).toContain('LIVE');
    expect(text).toContain('Live Room');
    expect(text).toContain('7 watching');
    expect(text).toContain('41 min left');
  });

  it('stays hidden when no room is live', async () => {
    await mount(fakeApi(live({ live: false })));
    expect(host.querySelector('.ck-live-bar')).toBeNull();
  });

  it('stays hidden when the status cannot be read (no desktop shell)', async () => {
    const api = fakeApi(live());
    api.status.mockRejectedValue(new Error('Desktop shell required: live_room_status'));
    await mount(api);
    expect(host.querySelector('.ck-live-bar')).toBeNull();
  });

  it('has no close button: only Pause, Copy link and Stop', async () => {
    await mount(fakeApi(live()));
    expect(buttons().map((b) => b.textContent?.trim())).toEqual(['Pause', 'Copy link', 'Stop']);
    expect(host.querySelector('[aria-label*="lose" i], [aria-label*="ismiss" i], [title*="lose" i]')).toBeNull();
  });

  it('Pause swaps to Resume and says viewers see Paused', async () => {
    const api = fakeApi(live());
    await mount(api);
    await click(button('Pause'));
    expect(api.pause).toHaveBeenCalledTimes(1);
    const text = host.textContent ?? '';
    expect(text).toContain('PAUSED');
    expect(text).toContain('viewers see "Paused"');
    expect(button('Pause')).toBeUndefined();
    expect(button('Resume')).toBeDefined();
    expect(buttons().map((b) => b.textContent?.trim())).toEqual(['Resume', 'Stop']);
    await click(button('Resume'));
    expect(api.resume).toHaveBeenCalledTimes(1);
    expect(button('Pause')).toBeDefined();
  });

  it('Stop calls the api, reports it, and the bar goes away', async () => {
    const api = fakeApi(live());
    const onStopped = vi.fn();
    await mount(api, onStopped);
    await click(button('Stop'));
    expect(api.stop).toHaveBeenCalledTimes(1);
    expect(onStopped).toHaveBeenCalledTimes(1);
    expect(host.querySelector('.ck-live-bar')).toBeNull();
  });

  it('Copy link fetches the link and writes it to the clipboard, never showing it', async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const api = fakeApi(live());
    await mount(api);
    await click(button('Copy link'));
    expect(api.copyLink).toHaveBeenCalledTimes(1);
    expect(writeText).toHaveBeenCalledWith(LINK);
    expect(button('Copied')).toBeDefined();
    expect(host.textContent ?? '').not.toContain('c2VjcmV0');
  });

  it('shows the app message when an action fails', async () => {
    const api = fakeApi(live());
    api.pause.mockRejectedValue(new Error('The room is busy.'));
    await mount(api);
    await click(button('Pause'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('The room is busy.');
  });
});
