// @vitest-environment jsdom
// Keys and scrolling for one computer: the state of every switch is readable, and each change goes out whole.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PeerPrefsPanel, speedLabel } from './PeerPrefsPanel';
import { DEFAULT_PEER_PREFS, type PeerPrefs } from '../../lib/desk-link';

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

function render(prefs?: Partial<PeerPrefs>) {
  const h = { onKeymap: vi.fn(), onScroll: vi.fn() };
  act(() => root.render(<PeerPrefsPanel name="Laptop" prefs={prefs ? { ...DEFAULT_PEER_PREFS, ...prefs } : undefined} {...h} />));
  return h;
}
const sw = (label: string) => host.querySelector<HTMLButtonElement>(`[role="switch"][aria-label="${label}"]`)!;
const speed = () => host.querySelector<HTMLSelectElement>('select')!;

describe('PeerPrefsPanel', () => {
  it('shows the defaults when the app has not said anything yet: swaps on, scrolling as is', () => {
    render();
    expect(host.textContent).toContain('Keys and scrolling with Laptop');
    expect(sw('Swap Alt and Command for Laptop').getAttribute('aria-checked')).toBe('true');
    expect(sw('Swap Windows key and Option for Laptop').getAttribute('aria-checked')).toBe('true');
    expect(sw('Reverse scrolling when I use Laptop').getAttribute('aria-checked')).toBe('false');
    expect(sw('Reverse scrolling when Laptop uses this computer').getAttribute('aria-checked')).toBe('false');
    expect(speed().value).toBe('1');
  });

  it('says in plain words what the swap does', () => {
    render();
    expect(host.textContent).toContain('Windows PC and the other a Mac');
  });

  it('turning a swap off sends both swaps, keeping the other as it was', () => {
    const h = render({ win_option: false });
    act(() => sw('Swap Alt and Command for Laptop').click());
    expect(h.onKeymap).toHaveBeenCalledWith(false, false);
    act(() => sw('Swap Windows key and Option for Laptop').click());
    expect(h.onKeymap).toHaveBeenLastCalledWith(true, true);
  });

  it('the scroll switches and the speed send all three scroll choices together', () => {
    const h = render({ speed: 1.5, reverse_incoming: true });
    act(() => sw('Reverse scrolling when I use Laptop').click());
    expect(h.onScroll).toHaveBeenLastCalledWith(true, 1.5, true);
    act(() => sw('Reverse scrolling when Laptop uses this computer').click());
    expect(h.onScroll).toHaveBeenLastCalledWith(false, 1.5, false);
    act(() => {
      speed().value = '2';
      speed().dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(h.onScroll).toHaveBeenLastCalledWith(false, 2, true);
  });

  it('a saved speed that is not on the list still shows, in order', () => {
    render({ speed: 1.25 });
    const values = [...host.querySelectorAll('option')].map((o) => o.value);
    expect(values).toEqual(['0.5', '0.75', '1', '1.25', '1.5', '2', '3']);
    expect(speed().value).toBe('1.25');
  });

  it('writes a speed as a multiplier', () => {
    expect(speedLabel(1)).toBe('1×');
    expect(speedLabel(0.75)).toBe('0.75×');
  });
});
