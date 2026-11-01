// @vitest-environment jsdom
// Add a computer: show a code (with a countdown), enter a code (errors stay until typing), found nearby (pick, then type).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AddComputerSheet, timeLeft } from './AddComputerSheet';
import type { PairHosting } from '../../lib/desk-pair';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** Test-only fixtures. */
const HOSTING: PairHosting = { code: '4F7Q-9K2M', ticket: 'pm1:' + 'ab'.repeat(32), expires_at: 1_000_000 + 105_000 };
const NOW = 1_000_000;

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

function render(over: Partial<Parameters<typeof AddComputerSheet>[0]> = {}) {
  const h = {
    onHostStart: vi.fn(async () => null),
    onHostCancel: vi.fn(),
    onJoin: vi.fn(async (): Promise<string | null> => null),
    onRefreshNearby: vi.fn(),
    onClose: vi.fn(),
  };
  const props = { hosting: null, nearby: [], joining: null, lockedUntil: null, now: () => NOW, ...h, ...over };
  act(() => root.render(<AddComputerSheet {...props} />));
  return h;
}
const q = (text: string) =>
  [...document.querySelectorAll('button:not([role="tab"])')].find((b) => b.textContent === text) as HTMLButtonElement;
const tab = (text: string) => [...document.querySelectorAll('[role="tab"]')].find((b) => b.textContent === text) as HTMLElement;
function type(el: HTMLInputElement, value: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  act(() => {
    set.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}
const inputs = () => [...document.querySelectorAll<HTMLInputElement>('input')];

describe('AddComputerSheet', () => {
  it('formats the time left', () => {
    expect(timeLeft(105_000)).toBe('1:45');
    expect(timeLeft(-5)).toBe('0:00');
  });

  it('asks the app to show a code', async () => {
    const h = render();
    await act(async () => q('Show a code').click());
    expect(h.onHostStart).toHaveBeenCalledTimes(1);
  });

  it('shows the code, how long it lasts and the address', () => {
    render({ hosting: HOSTING });
    const text = document.querySelector('[role="dialog"]')!.textContent!;
    expect(text).toContain('4F7Q-9K2M');
    expect(text).toContain('1:45');
    expect(inputs()[0].value).toBe(HOSTING.ticket);
  });

  it('an old code is not shown; a new one can be asked for', () => {
    render({ hosting: { ...HOSTING, expires_at: NOW - 1 } });
    expect(document.body.textContent).not.toContain('4F7Q-9K2M');
    expect(document.body.textContent).toContain('That code ended');
    expect(q('Show a new code')).toBeTruthy();
  });

  it('closing while a code is shown withdraws it', () => {
    const h = render({ hosting: HOSTING });
    act(() => q('Cancel').click());
    expect(h.onHostCancel).toHaveBeenCalledTimes(1);
    expect(h.onClose).toHaveBeenCalledTimes(1);
  });

  it('enter a code: checks the form first, then joins', async () => {
    const h = render();
    act(() => tab('Enter a code').click());
    await act(async () => q('Pair').click());
    expect(document.querySelector('[role="alert"]')!.textContent).toContain('Paste the address');
    expect(h.onJoin).not.toHaveBeenCalled();
    const [address, code] = inputs();
    type(address, 'pm1:abc');
    expect(document.querySelector('[role="alert"]')).toBeNull(); // typing clears it
    type(code, '4f7q-9k2m');
    await act(async () => q('Pair').click());
    expect(h.onJoin).toHaveBeenCalledWith('pm1:abc', '4F7Q-9K2M');
  });

  it('a refused code is shown in the sheet and stays until the person types', async () => {
    render({ onJoin: vi.fn(async () => 'The code was not accepted.') });
    act(() => tab('Enter a code').click());
    const [address, code] = inputs();
    type(address, 'pm1:abc');
    type(code, '4F7Q-9K2M');
    await act(async () => q('Pair').click());
    expect(document.querySelector('[role="alert"]')!.textContent).toBe('The code was not accepted.');
    await act(async () => new Promise((r) => setTimeout(r, 1200))); // a refresh tick later
    expect(document.querySelector('[role="alert"]')!.textContent).toBe('The code was not accepted.');
    type(code, '4F7Q-9K2');
    expect(document.querySelector('[role="alert"]')).toBeNull();
  });

  it('shows progress next to the button while pairing', async () => {
    let finish: (v: string | null) => void = () => {};
    render({ onJoin: () => new Promise((r) => (finish = r)), joining: 'Waiting for Ren-PC to confirm…' });
    act(() => tab('Enter a code').click());
    const [address, code] = inputs();
    type(address, 'pm1:abc');
    type(code, '4F7Q-9K2M');
    act(() => void q('Pair').click());
    expect(document.querySelector('[role="status"]')!.textContent).toBe('Waiting for Ren-PC to confirm…');
    await act(async () => finish(null));
  });

  it('found nearby: lists computers, picking one asks for its code only', async () => {
    const h = render({ nearby: [{ node: 'cd'.repeat(32), name: 'Ren-PC' }] });
    act(() => tab('Found nearby').click());
    expect(h.onRefreshNearby).toHaveBeenCalled();
    act(() => q('Ren-PC').click());
    expect(document.body.textContent).toContain('Pair with Ren-PC');
    expect(inputs()).toHaveLength(1);
    type(inputs()[0], '4F7Q9K2M');
    await act(async () => q('Pair').click());
    expect(h.onJoin).toHaveBeenCalledWith('cd'.repeat(32), '4F7Q9K2M');
  });

  it('found nearby with nobody: says how to be found, with Look again', () => {
    const h = render();
    act(() => tab('Found nearby').click());
    expect(document.body.textContent).toContain('No computer is showing a code nearby');
    act(() => q('Look again').click());
    expect(h.onRefreshNearby).toHaveBeenCalledTimes(2);
  });

  it('while pairing is paused the buttons are off and the sheet says until when', () => {
    render({ lockedUntil: NOW + 60_000 });
    expect(document.querySelector('[role="alert"]')!.textContent).toContain('Pairing is paused');
    expect((q('Show a code') as HTMLButtonElement).disabled).toBe(true);
  });
});
