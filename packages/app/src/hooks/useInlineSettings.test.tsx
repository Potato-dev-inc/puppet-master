// @vitest-environment jsdom
// Opening Settings from anywhere in the cockpit with a row name ("live-updates") lands on that row's page, and
// carries the row to the panel so it scrolls there. Individual mode hands the name to the Settings window.
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useInlineSettings, type InlineSettings } from './useInlineSettings';

vi.mock('../lib/cockpit-api', () => ({
  cockpitApi: { onSettingsOpen: () => Promise.resolve(() => {}) },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type View = 'pane' | 'settings';
let host: HTMLDivElement;
let root: Root;
let hook: InlineSettings;
let view: View;
let mode: 'orchestration' | 'individual';
const openWindow = vi.fn();

function Probe() {
  hook = useInlineSettings<View>({ mode, view, setView: (v) => (view = v), home: 'pane', openWindow });
  return null;
}
const render = () => act(async () => root.render(createElement(Probe)));

beforeEach(async () => {
  view = 'pane';
  mode = 'orchestration';
  openWindow.mockClear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await render();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('useInlineSettings with row names', () => {
  it('a row id opens that row\'s section, with the row to scroll to', async () => {
    await act(async () => hook.open('live-updates'));
    await render();
    expect(view).toBe('settings');
    expect(hook.viewProps.initialTab).toBe('general');
    expect(hook.viewProps.initialRow).toBe('live-updates');
  });

  it('a page name carries no row; a legacy tab name with a row (team) does', async () => {
    await act(async () => hook.open('keys'));
    await render();
    expect(hook.viewProps.initialTab).toBe('keys');
    expect(hook.viewProps.initialRow).toBeUndefined();
    await act(async () => hook.open('team'));
    await render();
    expect(hook.viewProps.initialRow).toBe('team');
  });

  it('asking again while Settings is open re-opens the panel on the new row (the view key changes)', async () => {
    await act(async () => hook.open('keys'));
    await render();
    const before = hook.viewKey;
    await act(async () => hook.open('budget-turns'));
    await render();
    expect(hook.viewKey).toBeGreaterThan(before);
    expect(hook.viewProps.initialTab).toBe('general');
    expect(hook.viewProps.initialRow).toBe('budget-turns');
  });

  it('Individual mode hands the name to the Settings window unchanged', async () => {
    mode = 'individual';
    await render();
    await act(async () => hook.open('live-updates'));
    expect(openWindow).toHaveBeenCalledWith('live-updates');
    expect(view).toBe('pane');
  });
});
