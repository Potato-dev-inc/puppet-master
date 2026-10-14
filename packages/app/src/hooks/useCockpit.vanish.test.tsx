// @vitest-environment jsdom
// A pane that closes while it is selected (a merge, a discard, Close) must never flash
// "Opening pane": the very first render after it left the list already shows the Inbox.
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useCockpit, type CockpitApi } from './useCockpit';

const registry = vi.hoisted(() => ({ ids: [] as string[] }));

vi.mock('./usePaneRegistry', () => ({
  usePaneRegistry: () => {
    const list = registry.ids.map((id) => ({ info: { id, agent_type: 'claude', cwd: '/p' }, status: 'idle' }));
    return { panes: new Map(list.map((p) => [p.info.id, p])), paneList: list, initialReady: true };
  },
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
  // Every command answers nothing; every `on…` subscription returns an unsubscribe.
  cockpitApi: new Proxy(
    {},
    {
      get: (_t, name: string) => (name.startsWith('on') ? () => Promise.resolve(() => {}) : () => Promise.resolve(null)),
    },
  ),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let api: CockpitApi;
const seen: (string | null)[] = [];

function Probe(_: { tick: number }) {
  api = useCockpit();
  seen.push(api.selectedId);
  return null;
}
let tick = 0;
const rerender = () => act(async () => root.render(createElement(Probe, { tick: ++tick })));

beforeEach(() => {
  registry.ids = ['p1', 'p2'];
  seen.length = 0;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('selection when the selected pane closes', () => {
  it('goes to the Inbox on the first render after the pane left the list', async () => {
    await rerender();
    await act(async () => api.select('p1'));
    expect(api.selectedId).toBe('p1');

    registry.ids = ['p2']; // the merge closed p1
    seen.length = 0;
    await rerender();
    expect(seen[0], 'no render may still show the closed pane (that is the "Opening pane" screen)').toBeNull();
    expect(api.selectedId).toBeNull();
  });

  it('a pane that is still opening (not listed yet) stays selected', async () => {
    await rerender();
    await act(async () => api.select('brand-new'));
    expect(api.selectedId).toBe('brand-new');
  });
});
