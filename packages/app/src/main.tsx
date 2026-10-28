/// <reference types="vite-plugin-pwa/client" />
import { StrictMode } from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import AskBarApp from './components/askbar/AskBarApp';
import PointBoxApp from './components/point/PointBoxApp';
import CockpitApp from './CockpitApp';
import PwaApp from './PwaApp';
import PwaLegacyApp from './PwaLegacyApp';
import SettingsApp from './SettingsApp';
import TerminalApp from './TerminalApp';
import { isTauriWindow, setupServiceWorker } from './lib/service-worker';
import { isSettingsEntry } from './lib/settings-entry';
import WorkerHostBootstrap from './WorkerHostBootstrap';
import './styles/index.css';

// Phone PWA only; inside the desktop app a service worker left by an older version is removed.
void setupServiceWorker({
  inDesktopApp: isTauriWindow(),
  container: 'serviceWorker' in navigator ? navigator.serviceWorker : undefined,
  cacheStorage: typeof caches === 'undefined' ? undefined : caches,
  register: () => import('virtual:pwa-register').then(({ registerSW }) => registerSW({ immediate: true })),
}).catch(() => undefined);

const params = new URLSearchParams(window.location.search);
const isTerminalApp = params.has('terminal') || params.has('cmd') || window.location.hash === '#terminal';
const isStandaloneWorker =
  params.has('worker') || import.meta.env.VITE_PUPPET_MASTER_WORKER === '1';
/** Backup of the old orchestrator-centric mobile UI. */
const isLegacyPwa = params.get('pwa') === 'legacy';
const isMobileOrPwa =
  params.has('pwa') ||
  window.matchMedia('(display-mode: standalone)').matches ||
  /Android|iPhone|iPad/i.test(navigator.userAgent);

async function resolveRoot(): Promise<typeof App> {
  // Dev-only UI evidence harness (tree-shaken from production builds).
  if (import.meta.env.DEV && params.has('harness')) {
    const { bootHarness } = await import('./dev/harness');
    return (await bootHarness(params.get('harness') ?? '')) as typeof App;
  }
  if (isSettingsEntry(window.location.search)) return SettingsApp;
  if (params.get('cockpit') === '1') return CockpitApp;
  if (params.get('askbar') === '1') return AskBarApp as typeof App;
  if (params.get('pointbox') === '1') return PointBoxApp as typeof App;
  if (isTerminalApp) return TerminalApp;
  if (isStandaloneWorker) return WorkerHostBootstrap;
  if (isMobileOrPwa) return isLegacyPwa ? PwaLegacyApp : PwaApp;
  try {
    const workerLaunch = await import('./lib/tauri').then((mod) => mod.tauri.getWorkerLaunch());
    if (workerLaunch) return WorkerHostBootstrap;
  } catch {
    /* browser preview */
  }
  return App;
}

// StrictMode is safe: TerminalSession defers terminal creation to the next
// animation frame so the first StrictMode mount can dispose before heavy
// renderer work is scheduled.
void resolveRoot().then((Root) => {
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <Root />
    </StrictMode>,
  );
});
