// @vitest-environment node
// The Point box route (`index.html?pointbox=1`) resolves to the Point box root, next to the askbar route.
// Node environment on purpose: main.tsx imports `virtual:pwa-register`, which only the app's Vite config
// (with the PWA plugin) can resolve; in node (SSR transform) that import is left for runtime and never runs here.
import { beforeEach, describe, expect, it, vi } from 'vitest';

// main.tsx reads the window and the document when it loads. Node has neither, so a small stand-in
// carries the query string; the test sets `location.search` before each load.
const dom = vi.hoisted(() => {
  const location = { search: '', hash: '' };
  Object.assign(globalThis, {
    window: {
      location,
      history: { replaceState: () => undefined },
      matchMedia: () => ({ matches: false }),
    },
    document: { getElementById: () => ({}) },
  });
  return { location };
});

// The element main.tsx hands to ReactDOM (`<StrictMode><Root /></StrictMode>`), captured by the mocked renderer.
const rendered = vi.hoisted(() => ({ element: null as unknown }));

vi.mock('react-dom/client', () => ({
  default: {
    createRoot: () => ({
      render: (element: { props: { children: unknown } }) => {
        rendered.element = element;
      },
    }),
  },
}));

// Heavy roots and desktop plumbing are replaced by empty components: only the route choice matters here.
vi.mock('./App', () => ({ default: () => null }));
vi.mock('./components/askbar/AskBarApp', () => ({ default: () => null }));
vi.mock('./CockpitApp', () => ({ default: () => null }));
vi.mock('./PwaApp', () => ({ default: () => null }));
vi.mock('./PwaLegacyApp', () => ({ default: () => null }));
vi.mock('./SettingsApp', () => ({ default: () => null }));
vi.mock('./TerminalApp', () => ({ default: () => null }));
vi.mock('./WorkerHostBootstrap', () => ({ default: () => null }));
vi.mock('./lib/service-worker', () => ({
  isTauriWindow: () => false,
  setupServiceWorker: async () => 'unsupported',
}));
vi.mock('./lib/tauri', () => ({ tauri: { getWorkerLaunch: async () => null } }));

/**
 * Loads main.tsx fresh for a query string. Returns the component it chose to render, and the
 * components of the same (fresh) module graph, so identity checks compare like with like.
 */
async function loadRouteFor(search: string) {
  vi.resetModules();
  dom.location.search = search;
  rendered.element = null;
  const { PointBoxApp } = await import('./components/point/PointBoxApp');
  const { default: App } = await import('./App');
  const { default: AskBarApp } = await import('./components/askbar/AskBarApp');
  await import('./main');
  await vi.waitFor(() => {
    expect(rendered.element).not.toBeNull();
  });
  const outer = rendered.element as { props: { children: { type: unknown } } } | null;
  if (!outer) throw new Error('main.tsx rendered nothing');
  const Root = outer.props.children.type;
  return { Root, PointBoxApp, App, AskBarApp };
}

describe('main.tsx point box route', () => {
  beforeEach(() => {
    dom.location.hash = '';
  });

  it('?pointbox=1 renders the Point box root', async () => {
    const { Root, PointBoxApp } = await loadRouteFor('?pointbox=1');
    expect(Root).toBe(PointBoxApp);
  });

  it('a plain window still renders the main app', async () => {
    const { Root, App } = await loadRouteFor('');
    expect(Root).toBe(App);
  });

  it('pointbox=0 does not switch to the Point box', async () => {
    const { Root, PointBoxApp } = await loadRouteFor('?pointbox=0');
    expect(Root).not.toBe(PointBoxApp);
  });

  it('the askbar route is unchanged next to it', async () => {
    const { Root, AskBarApp } = await loadRouteFor('?askbar=1');
    expect(Root).toBe(AskBarApp);
  });
});
