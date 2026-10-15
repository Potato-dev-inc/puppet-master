// @vitest-environment jsdom
// The Simple mode switch is OFF unless the setting says so, so the normal cockpit is unchanged.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const settings = vi.hoisted(() => ({ value: {} as Record<string, unknown> }));
vi.mock('./settings', () => ({ loadSettings: async () => settings.value }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  settings.value = {};
  window.localStorage.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

async function mountHook(): Promise<() => boolean> {
  const { useSimpleMode } = await import('./simple-mode');
  let current = false;
  function Probe() {
    current = useSimpleMode();
    return null;
  }
  await act(async () => {
    root.render(<Probe />);
  });
  await act(async () => {});
  return () => current;
}

describe('useSimpleMode', () => {
  it('is off when the setting is missing (the default)', async () => {
    expect((await mountHook())()).toBe(false);
  });

  it('is off when the setting is false', async () => {
    settings.value = { simple_mode: false };
    expect((await mountHook())()).toBe(false);
  });

  it('is on when the setting is true, and remembers it for the first frame next time', async () => {
    settings.value = { simple_mode: true };
    expect((await mountHook())()).toBe(true);
    expect(window.localStorage.getItem('pm-simple-mode')).toBe('1');
  });

  it('a wrong value in the file does not turn it on', async () => {
    settings.value = { simple_mode: 'yes' };
    expect((await mountHook())()).toBe(false);
  });

  it('is a switch in Settings > General, off by default', async () => {
    const { generalRows } = await import('../components/settings/sections/GeneralSection');
    const row = generalRows.find((r) => r.id === 'simple-mode');
    expect(row?.label).toBe('Simple mode: fewer buttons and plain words');
  });
});
