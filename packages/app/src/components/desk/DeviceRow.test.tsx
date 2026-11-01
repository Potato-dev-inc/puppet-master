// @vitest-environment jsdom
// One paired computer: words for its state, three switches that start off, Remove asks first.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DeviceRow, deviceStatus } from './DeviceRow';
import type { PairDevice } from '../../lib/desk-pair';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** Test-only fixture. */
const LAPTOP: PairDevice = { node: 'ab'.repeat(32), name: 'Laptop', added_at: 1, online: null, allowed: [] };

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

function render(device: Partial<PairDevice> = {}) {
  const h = { onToggle: vi.fn(), onRemove: vi.fn() };
  act(() => root.render(<ul><DeviceRow device={{ ...LAPTOP, ...device }} {...h} /></ul>));
  return h;
}
const button = (label: string) => host.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;

describe('DeviceRow', () => {
  it('says its state in words', () => {
    expect(deviceStatus(true)).toBe('Connected');
    expect(deviceStatus(false)).toBe('Offline');
    expect(deviceStatus(null)).toBe('Paired');
    render({ online: false });
    expect(host.textContent).toContain('Laptop');
    expect(host.querySelector('[role="status"]')!.textContent).toBe('Offline');
  });

  it('shows the three switches, all off for a new computer', () => {
    render();
    for (const name of ['Mouse and keyboard', 'Clipboard', 'Files']) {
      expect(button(`${name} for Laptop`).getAttribute('aria-checked')).toBe('false');
    }
  });

  it('a switch asks for the opposite of what it shows', () => {
    const h = render({ allowed: ['files'] });
    act(() => button('Mouse and keyboard for Laptop').click());
    expect(h.onToggle).toHaveBeenCalledWith('desk/input', true);
    act(() => button('Files for Laptop').click());
    expect(h.onToggle).toHaveBeenCalledWith('files', false);
  });

  it('Remove asks first; Keep goes back and nothing is removed', () => {
    const h = render();
    act(() => button('Remove Laptop').click());
    expect(host.textContent).toContain("Remove Laptop? It can't control this computer until you pair again.");
    expect(h.onRemove).not.toHaveBeenCalled();
    const keep = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Keep')!;
    act(() => keep.click());
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    expect(h.onRemove).not.toHaveBeenCalled();
  });

  it('confirming Remove removes', () => {
    const h = render();
    act(() => button('Remove Laptop').click());
    const remove = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Remove')!;
    act(() => remove.click());
    expect(h.onRemove).toHaveBeenCalledTimes(1);
  });
});
