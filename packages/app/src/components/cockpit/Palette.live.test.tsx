// @vitest-environment jsdom
// The palette's Live Room rows: "Start Live Room…" while there is no room, "Stop Live Room" only while one runs,
// and neither where the cockpit does not host a Live Room.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { GO_LIVE_EVENT, liveRoomApi, type LiveStatus } from '../../lib/live-room';
import { Palette } from './Palette';
import { LiveStatusContext } from './live/useLiveRoom';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

const cockpit = (over: Partial<CockpitApi> = {}) =>
  ({
    registry: { paneList: [], panes: new Map() },
    launchEntries: [],
    readyAgents: new Set(),
    names: new Map(),
    selectedId: 'p7',
    mode: 'orchestration',
    theme: 'dark',
    previews: {},
    openSettings: vi.fn(async () => {}),
    notify: vi.fn(),
    ...over,
  }) as unknown as CockpitApi;

const room = (over: Partial<LiveStatus> = {}): LiveStatus => ({
  live: true,
  paused: false,
  viewers: 3,
  endsAtMs: Date.now() + 40 * 60_000,
  paneIds: ['p7'],
  hidden: 0,
  stopped: null,
  ...over,
});

const titles = () => [...host.querySelectorAll('.ck-palette-title')].map((n) => n.textContent);
const row = (title: string) => [...host.querySelectorAll<HTMLElement>('[role="option"]')].find((r) => r.querySelector('.ck-palette-title')?.textContent === title);

async function open(status: LiveStatus | null | undefined, c = cockpit(), onClose = () => {}) {
  await act(async () =>
    root.render(
      <LiveStatusContext.Provider value={status}>
        <Palette c={c} onClose={onClose} />
      </LiveStatusContext.Provider>,
    ),
  );
}

describe('palette: Live Room', () => {
  it('offers Start, not Stop, when no room runs', async () => {
    await open(room({ live: false, stopped: 'owner' }));
    expect(titles()).toContain('Start Live Room…');
    expect(titles()).not.toContain('Stop Live Room');
    await open(null);
    expect(titles()).toContain('Start Live Room…');
  });

  it('offers Stop, not Start, only while a room runs (also while paused)', async () => {
    await open(room());
    expect(titles()).toContain('Stop Live Room');
    expect(titles()).not.toContain('Start Live Room…');
    await open(room({ paused: true }));
    expect(titles()).toContain('Stop Live Room');
  });

  it('shows no Live Room row where the cockpit hosts none', async () => {
    await open(undefined);
    expect(titles().some((t) => t?.includes('Live Room'))).toBe(false);
  });

  it('Start closes the palette and asks the cockpit to open the sheet for the selected pane', async () => {
    const seen: Array<{ paneId: string | null }> = [];
    const on = (e: Event) => seen.push((e as CustomEvent<{ paneId: string | null }>).detail);
    window.addEventListener(GO_LIVE_EVENT, on);
    const onClose = vi.fn();
    await open(null, cockpit(), onClose);
    act(() => row('Start Live Room…')!.click());
    window.removeEventListener(GO_LIVE_EVENT, on);
    expect(onClose).toHaveBeenCalled();
    expect(seen).toEqual([{ paneId: 'p7' }]);
  });

  it('Stop ends the room through the app, and says so when it cannot', async () => {
    const stop = vi.spyOn(liveRoomApi, 'stop').mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('Relay did not answer'));
    const c = cockpit();
    await open(room(), c);
    await act(async () => row('Stop Live Room')!.click());
    expect(stop).toHaveBeenCalledTimes(1);
    expect(c.notify).not.toHaveBeenCalled();
    await open(room(), c);
    await act(async () => row('Stop Live Room')!.click());
    expect(c.notify).toHaveBeenCalledWith('Relay did not answer');
  });
});
