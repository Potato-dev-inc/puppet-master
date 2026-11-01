// @vitest-environment jsdom
// The screens drawn to scale: click hops, the pointer dot, the find ring (1.5 s, only for a new find), the empty state.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FIND_RING_MS, ScreensView, boundsOf } from './ScreensView';
import type { DeskFind, DeskScreen } from '../../lib/desk';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SCREENS: DeskScreen[] = [
  { id: 'a', name: 'A', x: 0, y: 0, width: 2560, height: 1440, scale: 1, primary: true, index: 1 },
  { id: 'b', name: 'B', x: 2560, y: 0, width: 1920, height: 1080, scale: 1, primary: false, index: 2 },
];
const find = (seq: number): DeskFind => ({ seq, x: 100, y: 100, screen_index: 1, announce: 'Pointer is here' });

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
  vi.useRealTimers();
});

function renderView(props: Partial<Parameters<typeof ScreensView>[0]> = {}) {
  const handlers = { onHop: vi.fn(), onFind: vi.fn(), onRetry: vi.fn() };
  const el = (over: Partial<Parameters<typeof ScreensView>[0]>) => (
    <ScreensView screens={SCREENS} pointer={null} find={null} {...handlers} {...props} {...over} />
  );
  act(() => root.render(el({})));
  return { handlers, rerender: (over: Partial<Parameters<typeof ScreensView>[0]>) => act(() => root.render(el(over))) };
}

describe('ScreensView', () => {
  it('draws each screen to scale, labelled with its size', () => {
    renderView();
    expect(host.textContent).toContain('Screen 1 · 2560×1440 · main');
    expect(host.textContent).toContain('Screen 2 · 1920×1080');
    const first = host.querySelector<HTMLElement>('[aria-label="Go to screen 1"]')!;
    expect(first.style.width).toBe('57.14285714285714%');
    expect(first.style.height).toBe('100%');
  });

  it('clicking a screen hops to its number', () => {
    const { handlers } = renderView();
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Go to screen 2"]')!.click());
    expect(handlers.onHop).toHaveBeenCalledWith(2);
  });

  it('Find my pointer calls onFind', () => {
    const { handlers } = renderView();
    act(() => [...host.querySelectorAll('button')].find((b) => b.textContent === 'Find my pointer')!.click());
    expect(handlers.onFind).toHaveBeenCalledTimes(1);
  });

  it('shows the pointer dot where the pointer is', () => {
    renderView({ pointer: { x: 0, y: 0, screen_id: 'a', screen_index: 1 } });
    expect(host.querySelector('.ck-desk-pointer')).not.toBeNull();
  });

  it('draws the find ring for a new find and removes it after 1.5 s', () => {
    vi.useFakeTimers();
    const { rerender } = renderView({ find: find(1) });
    expect(host.querySelector('.ck-desk-ring')).toBeNull(); // the find seen at mount does not ring
    rerender({ find: find(2) });
    expect(host.querySelector('.ck-desk-ring')).not.toBeNull();
    expect(host.querySelector('[role="status"]')?.textContent).toBe('Pointer is here');
    act(() => vi.advanceTimersByTime(FIND_RING_MS - 10));
    expect(host.querySelector('.ck-desk-ring')).not.toBeNull();
    act(() => vi.advanceTimersByTime(20));
    expect(host.querySelector('.ck-desk-ring')).toBeNull();
  });

  it('an empty list says so, and Try again asks for a refresh', () => {
    const { handlers } = renderView({ screens: [] });
    expect(host.textContent).toContain('No screens found. Plug in a screen, then try again.');
    act(() => [...host.querySelectorAll('button')].find((b) => b.textContent === 'Try again')!.click());
    expect(handlers.onRetry).toHaveBeenCalledTimes(1);
  });

  it('boundsOf covers every screen', () => {
    expect(boundsOf(SCREENS)).toEqual({ minX: 0, minY: 0, width: 4480, height: 1440 });
  });
});
