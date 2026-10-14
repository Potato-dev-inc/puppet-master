// @vitest-environment jsdom
// `openSettings` is the cockpit's way to ask Rust for the Settings window (Individual mode, and the fallback
// when a page names none): it passes the page along and tells the person when the window did not open.
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useCockpit, type CockpitApi } from './useCockpit';

const open = vi.hoisted(() => ({ fn: null as unknown as (tab?: string) => Promise<void> }));

vi.mock('./usePaneRegistry', () => ({
  usePaneRegistry: () => ({ panes: new Map(), paneList: [], initialReady: true }),
}));
vi.mock('../lib/agentRegistry', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../lib/agentRegistry')>()),
  useAgentRegistry: () => ({ agents: [], dispatchable_ids: [] }),
}));
vi.mock('../lib/pane-preview', () => ({ usePanePreviews: () => ({}) }));
vi.mock('../lib/pane-activity', () => ({ usePaneActivity: () => ({}) }));
vi.mock('./useBridge', () => ({ useBridge: () => ({ isReady: false, error: null, client: null }) }));
vi.mock('./useQuestions', () => ({ useQuestions: () => ({ questions: [], pending: 0 }) }));
vi.mock('../lib/settings', () => ({
  applyTheme: () => {},
  loadSettings: () => Promise.resolve({}),
  patchSettings: () => Promise.resolve(),
}));
vi.mock('../lib/settings-sync', () => ({ subscribeSettingsTheme: () => () => {} }));
vi.mock('../lib/tauri', () => ({ tauri: { standbyGetStatus: () => Promise.resolve(null) } }));
vi.mock('../lib/cockpit-api', () => ({
  cockpitApi: new Proxy(
    {},
    {
      get: (_t, name: string) => {
        if (name === 'openSettings') return (tab?: string) => open.fn(tab);
        return name.startsWith('on') ? () => Promise.resolve(() => {}) : () => Promise.resolve(null);
      },
    },
  ),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let api: CockpitApi;

function Probe() {
  api = useCockpit();
  return null;
}

beforeEach(async () => {
  open.fn = vi.fn(async () => {});
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root.render(createElement(Probe)));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe('openSettings (the Settings window path)', () => {
  it('passes the page to Rust', async () => {
    await act(async () => api.openSettings('bridge'));
    expect(open.fn).toHaveBeenCalledWith('bridge');
  });

  it('opens on no page when none is named', async () => {
    await act(async () => api.openSettings());
    expect(open.fn).toHaveBeenCalledWith(undefined);
  });

  it('says so when the window did not open, and logs why', async () => {
    open.fn = vi.fn(async () => {
      throw new Error('settings window failed');
    });
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    await act(async () => api.openSettings('bridge'));
    expect(logged).toHaveBeenCalledWith('open_settings failed', expect.any(Error));
    expect(api.toasts.map((t) => t.text).join(' ')).toContain('settings window failed');
  });
});
