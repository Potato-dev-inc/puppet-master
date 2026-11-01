// @vitest-environment jsdom
// Open Settings on an exact row: it opens the tab, scrolls the row into view, glows it for 2 s, and can focus its field.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SETTINGS_GLOW_MS, openSettingsAt } from './openSettingsAt';

function addRow(id: string, withInput = false) {
  const el = document.createElement('div');
  el.className = 'st-rowwrap';
  el.setAttribute('data-row', id);
  if (withInput) el.innerHTML = '<input type="password" aria-label="key" />';
  document.body.appendChild(el);
  return el;
}

describe('openSettingsAt', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    document.body.innerHTML = '';
    Element.prototype.scrollIntoView = vi.fn();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('opens the section tab, or the row id when one is given', () => {
    const open = vi.fn();
    openSettingsAt('potato', 'potato-key', { open });
    expect(open).toHaveBeenLastCalledWith('potato-key');
    openSettingsAt('potato', undefined, { open });
    expect(open).toHaveBeenLastCalledWith('potato');
  });

  it('scrolls the row into view and glows it for 2 s, then clears the glow', () => {
    const row = addRow('potato-limit');
    openSettingsAt('potato', 'potato-limit', { open: vi.fn() });
    expect(row.getAttribute('data-glow')).toBe('true');
    expect(row.scrollIntoView).toHaveBeenCalledWith(expect.objectContaining({ block: 'center' }));
    vi.advanceTimersByTime(SETTINGS_GLOW_MS - 1);
    expect(row.getAttribute('data-glow')).toBe('true');
    vi.advanceTimersByTime(1);
    expect(row.hasAttribute('data-glow')).toBe(false);
  });

  it('finds a row that mounts a moment later', () => {
    const open = vi.fn();
    openSettingsAt('potato', 'potato-key', { open });
    const row = addRow('potato-key');
    vi.advanceTimersByTime(100);
    expect(row.getAttribute('data-glow')).toBe('true');
  });

  it('puts the cursor in the first text field when asked', () => {
    const row = addRow('potato-key', true);
    openSettingsAt('potato', 'potato-key', { open: vi.fn(), focusField: true });
    expect(document.activeElement).toBe(row.querySelector('input'));
  });

  it('dispatches pm:open-settings when no opener is given', () => {
    const seen: unknown[] = [];
    const listener = (e: Event) => seen.push((e as CustomEvent).detail);
    window.addEventListener('pm:open-settings', listener);
    openSettingsAt('potato', 'potato-key');
    window.removeEventListener('pm:open-settings', listener);
    expect(seen).toEqual([{ tab: 'potato-key' }]);
  });
});
