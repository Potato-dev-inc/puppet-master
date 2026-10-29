// @vitest-environment jsdom
// The "Review N" button in the sidebar: the count comes from review_desk_count, is hidden at 0, and follows the refresh events.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { ShipApi } from '../../lib/ship-api';
import { Sidebar } from './Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const review = vi.hoisted(() => ({
  count: undefined as unknown as ReturnType<typeof vi.fn>,
  refresh: null as null | (() => void),
  off: undefined as unknown as ReturnType<typeof vi.fn>,
}));
vi.mock('../../lib/review-desk', async (importOriginal) => {
  review.count = vi.fn();
  review.off = vi.fn();
  return {
    ...(await importOriginal<typeof import('../../lib/review-desk')>()),
    reviewDeskCount: () => review.count(),
    onReviewDeskRefresh: (cb: () => void) => {
      review.refresh = cb;
      return review.off;
    },
  };
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  review.count.mockReset();
  review.off.mockReset();
  review.refresh = null;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function fakeApi(): CockpitApi {
  const info = { id: 'p1', agent_type: 'codex', pid: 1, status: 'idle', created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 80, rows: 24, title: null } as unknown as PaneInfo;
  const panes = [{ info, status: 'idle' }];
  return {
    registry: { paneList: panes, panes: new Map([['p1', panes[0]]]), initialReady: true },
    names: new Map([['p1', 'Codex']]),
    asked: { questions: [] },
    launchEntries: [{ type: 'codex', label: 'Codex' }],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: 'p1',
    select: vi.fn(),
    prompts: {},
    statusSince: new Map([['p1', 0]]),
    now: 60_000,
    spawn: null,
    toasts: [],
    notify: vi.fn(),
    openSettings: vi.fn(async () => {}),
    bridgeLine: '',
  } as unknown as CockpitApi;
}

const ship = { recent: vi.fn().mockResolvedValue([]), gitState: vi.fn().mockResolvedValue('repo') } as unknown as ShipApi;

async function render(onOpenReview?: () => void) {
  await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} onOpenReview={onOpenReview} ship={ship} />));
  await act(async () => {});
}
const button = () => [...host.querySelectorAll('button')].find((b) => b.textContent?.startsWith('Review'));

describe('sidebar: Review', () => {
  it('shows "Review 7" when seven things wait, and opens the desk on click', async () => {
    review.count.mockResolvedValue(7);
    const open = vi.fn();
    await render(open);
    expect(button()?.textContent).toBe('Review 7');
    act(() => button()!.click());
    expect(open).toHaveBeenCalledTimes(1);
  });

  it('shows plain "Review" with no number at 0', async () => {
    review.count.mockResolvedValue(0);
    await render(() => {});
    expect(button()?.textContent).toBe('Review');
    expect(host.querySelector('.ck-side-count')).toBeNull();
  });

  it('shows no number when the count cannot be read', async () => {
    review.count.mockRejectedValue(new Error('Desktop shell required'));
    await render(() => {});
    expect(button()?.textContent).toBe('Review');
  });

  it('has no button, and reads no count, without an open handler', async () => {
    review.count.mockResolvedValue(3);
    await render();
    expect(button()).toBeUndefined();
    expect(review.count).not.toHaveBeenCalled();
  });

  it('reads the count again on a refresh event', async () => {
    review.count.mockResolvedValue(2);
    await render(() => {});
    expect(button()?.textContent).toBe('Review 2');
    review.count.mockResolvedValue(5);
    await act(async () => review.refresh?.());
    expect(button()?.textContent).toBe('Review 5');
  });
});
