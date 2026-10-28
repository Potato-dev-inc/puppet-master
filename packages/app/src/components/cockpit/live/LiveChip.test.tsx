// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import type { LiveRoomApi, LiveStatus } from '../../../lib/live-room';
import { LiveChip } from './LiveChip';

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

const room = (over: Partial<LiveStatus> = {}): LiveStatus => ({
  live: true,
  paused: false,
  viewers: 2,
  endsAtMs: Date.now() + 60 * 60_000,
  paneIds: ['p-shared'],
  hidden: 0,
  stopped: null,
  ...over,
});

type MockedApi = LiveRoomApi & { status: Mock; pause: Mock; resume: Mock; stop: Mock };

function fakeApi(): MockedApi {
  return {
    status: vi.fn(),
    pause: vi.fn(async () => {}),
    resume: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
  } as unknown as MockedApi;
}

async function render(props: { paneId: string; status: LiveStatus | null; api: LiveRoomApi; onStopped?: () => void }) {
  await act(async () => {
    root.render(<LiveChip {...props} />);
  });
}

const chip = () => host.querySelector('.ck-live-chip') as HTMLButtonElement | null;
const menuItem = (text: string) =>
  Array.from(host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find((b) => b.textContent === text);
const click = async (el: HTMLElement | null | undefined) => {
  await act(async () => {
    el?.click();
  });
};

describe('LiveChip', () => {
  it('renders only for a pane that is in the room', async () => {
    await render({ paneId: 'p-other', status: room(), api: fakeApi() });
    expect(chip()).toBeNull();
  });

  it('renders nothing when no room is live', async () => {
    await render({ paneId: 'p-shared', status: room({ live: false }), api: fakeApi() });
    expect(chip()).toBeNull();
  });

  it('shows the LIVE chip on a shared pane', async () => {
    await render({ paneId: 'p-shared', status: room(), api: fakeApi() });
    expect(chip()?.textContent).toBe('LIVE');
    expect(chip()?.getAttribute('aria-expanded')).toBe('false');
  });

  it('the menu offers Pause and Stop', async () => {
    await render({ paneId: 'p-shared', status: room(), api: fakeApi() });
    await click(chip());
    expect(menuItem('Pause')).toBeDefined();
    expect(menuItem('Stop')).toBeDefined();
    expect(menuItem('Resume')).toBeUndefined();
  });

  it('shows Resume instead of Pause while paused', async () => {
    await render({ paneId: 'p-shared', status: room({ paused: true }), api: fakeApi() });
    await click(chip());
    expect(menuItem('Resume')).toBeDefined();
    expect(menuItem('Pause')).toBeUndefined();
  });

  it('Pause in the menu calls the api and closes the menu', async () => {
    const api = fakeApi();
    await render({ paneId: 'p-shared', status: room(), api });
    await click(chip());
    await click(menuItem('Pause'));
    expect(api.pause).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[role="menu"]')).toBeNull();
  });

  it('Stop in the menu calls the api and reports it', async () => {
    const api = fakeApi();
    const onStopped = vi.fn();
    await render({ paneId: 'p-shared', status: room(), api, onStopped });
    await click(chip());
    await click(menuItem('Stop'));
    expect(api.stop).toHaveBeenCalledTimes(1);
    expect(onStopped).toHaveBeenCalledTimes(1);
  });

  it('reads the status itself when none is passed in', async () => {
    const api = fakeApi();
    api.status.mockResolvedValue(room());
    await act(async () => {
      root.render(<LiveChip paneId="p-shared" api={api} subscribe={async () => () => {}} />);
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    expect(api.status).toHaveBeenCalled();
    expect(chip()?.textContent).toBe('LIVE');
  });
});
