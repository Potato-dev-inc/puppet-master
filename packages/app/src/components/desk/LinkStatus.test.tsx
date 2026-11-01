// @vitest-environment jsdom
// The mouse line: words, one button that fits the moment, nothing while nothing is shared.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LinkStatus } from './LinkStatus';
import type { LinkView } from '../../lib/desk-link';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** Test-only fixture. */
function view(over: Partial<LinkView> = {}): LinkView {
  return {
    seq: 1,
    devices: [{ node: 'ab'.repeat(32), name: 'Laptop', state: 'online', side: 'right', mouse_here: false, controls_me: false, input_on: true, clipboard_on: true }],
    mouse_on: null,
    controlled_by: null,
    paused: false,
    sharing: true,
    tray: 'Mouse sharing is on',
    admin_note: "Can't control windows that run as admin.",
    error: null,
    clipboard_note: null,
    supported: true,
    ...over,
  };
}

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

function render(v: LinkView | null, error: string | null = null) {
  const h = { onPause: vi.fn(), onPanic: vi.fn() };
  act(() => root.render(<LinkStatus view={v} error={error} {...h} />));
  return h;
}
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text);

describe('LinkStatus', () => {
  it('draws nothing before the link answers', () => {
    render(null);
    expect(host.textContent).toBe('');
  });

  it('says "Mouse is on Laptop" when the pointer left, and offers to bring it back', () => {
    const h = render(view({ mouse_on: 'Laptop' }));
    expect(host.querySelector('[role="status"]')!.textContent).toContain('Mouse is on Laptop');
    expect(button('Pause sharing')).toBeUndefined();
    act(() => button('Bring the mouse back')!.click());
    expect(h.onPanic).toHaveBeenCalledTimes(1);
  });

  it('changes with the state: on, then on the other computer, then paused', () => {
    const none = { onPause: () => {}, onPanic: () => {} };
    render(view());
    expect(host.textContent).toContain('Mouse sharing is on · Laptop connected');
    act(() => root.render(<LinkStatus view={view({ mouse_on: 'Laptop' })} error={null} {...none} />));
    expect(host.textContent).toContain('Mouse is on Laptop');
    act(() => root.render(<LinkStatus view={view({ paused: true })} error={null} {...none} />));
    expect(host.textContent).toContain('Mouse sharing is paused');
    expect(button('Resume sharing')).toBeDefined();
  });

  it('pauses and resumes', () => {
    const h = render(view());
    act(() => button('Pause sharing')!.click());
    expect(h.onPause).toHaveBeenCalledWith(true);
    const h2 = render(view({ paused: true }));
    act(() => button('Resume sharing')!.click());
    expect(h2.onPause).toHaveBeenCalledWith(false);
  });

  it('shows an error as an alert, and a copy that did not go across', () => {
    render(view({ error: 'Could not take the mouse: x', clipboard_note: 'That copy is too big to share between computers (32 MB limit).' }), 'Pick left, right, above or below.');
    const alerts = [...host.querySelectorAll('[role="alert"]')].map((a) => a.textContent ?? '');
    expect(alerts.some((t) => t.includes('Mouse sharing hit a problem'))).toBe(true);
    expect(alerts.some((t) => t.includes('Pick left, right, above or below.'))).toBe(true);
    expect(host.textContent).toContain('32 MB limit');
  });

  it('says nothing while nothing is shared', () => {
    render(view({ sharing: false, tray: null, devices: [] }));
    expect(host.querySelector('[data-testid="link-status"]')).toBeNull();
  });
});
