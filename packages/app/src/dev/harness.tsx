// Dev-only UI evidence harness: `?harness=<scenario>` renders the real cockpit/settings UI in a
// plain browser with Tauri mocked. Imported only behind `import.meta.env.DEV` (see main.tsx).
import AskBarApp from '../components/askbar/AskBarApp';
import CockpitApp from '../CockpitApp';
import PwaApp from '../PwaApp';
import SettingsApp from '../SettingsApp';
import { installMockBridge } from './mock-bridge';
import { blockNetwork } from './block-network';
import { installTauriMock } from './mock-tauri';
import { scenarios } from './scenarios';
import type { HarnessDom, HarnessScenario } from './types';

const sleep = (ms: number) => new Promise<void>((r) => window.setTimeout(r, ms));

const dom: HarnessDom = {
  sleep,
  async waitFor(selector, timeoutMs = 4000) {
    const end = Date.now() + timeoutMs;
    while (Date.now() < end) {
      const el = document.querySelector(selector);
      if (el) return el;
      await sleep(50);
    }
    return null;
  },
  async click(selector) {
    const el = await dom.waitFor(selector);
    if (!(el instanceof HTMLElement)) return false;
    el.click();
    return true;
  },
  async clickText(selector, text) {
    const end = Date.now() + 4000;
    while (Date.now() < end) {
      const el = [...document.querySelectorAll<HTMLElement>(selector)].find((e) => e.textContent?.includes(text));
      if (el) {
        el.click();
        return true;
      }
      await sleep(50);
    }
    return false;
  },
  key(key, mods = {}) {
    window.dispatchEvent(
      new KeyboardEvent('keydown', { key, ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift, bubbles: true, cancelable: true }),
    );
  },
};

function UnknownScenario({ name }: { name: string }) {
  return (
    <pre style={{ padding: 24, color: '#f55' }}>
      Unknown harness scenario "{name}". Available: {Object.keys(scenarios).join(', ')}
    </pre>
  );
}

/** Installs the mocks, shapes the URL the app reads, and returns the root component. */
export async function bootHarness(name: string): Promise<() => JSX.Element> {
  const scenario: HarnessScenario | undefined = scenarios[name];
  if (!scenario) return () => <UnknownScenario name={name} />;

  blockNetwork();
  // Screenshots are taken under virtual time, so a CSS fade can be caught half way. Land every
  // animation and transition on its final frame so the PNG shows the settled UI.
  const settle = document.createElement('style');
  settle.textContent = '*,*::before,*::after{animation-duration:0s!important;animation-delay:0s!important;transition-duration:0s!important;transition-delay:0s!important}';
  document.head.appendChild(settle);
  if (scenario.route === 'phone') {
    installMockBridge(scenario);
  } else {
    installTauriMock(scenario);
  }
  const q = new URLSearchParams();
  if (scenario.route === 'phone') {
    q.set('pwa', '1');
  } else if (scenario.route === 'settings') {
    q.set('settings', '1');
    if (scenario.settingsTab) q.set('tab', scenario.settingsTab);
  } else if (scenario.route === 'askbar') {
    q.set('askbar', '1');
  } else {
    q.set('cockpit', '1');
    // The shooter reuses one browser tab, so whether this looks like a first run must not depend on the scenario before it.
    try {
      if (scenario.firstRun) window.localStorage.removeItem('pm.cockpit.everSpawned');
      else window.localStorage.setItem('pm.cockpit.everSpawned', '1');
    } catch {
      /* storage blocked: the first-run hint stays off */
    }
    if (scenario.select) q.set('select', scenario.select);
  }
  window.history.replaceState(null, '', `${window.location.pathname}?${q.toString()}`);
  document.documentElement.dataset.theme = scenario.theme ?? 'dark';

  // Screenshots that are not taken under virtual time (narrow phone viewports) wait for this flag.
  (window as unknown as { __harnessReady?: boolean }).__harnessReady = false;
  const ready = () => {
    (window as unknown as { __harnessReady?: boolean }).__harnessReady = true;
  };
  if (scenario.after) {
    // Let the first render and mocked list_panes settle, then drive the UI.
    window.setTimeout(() => {
      void Promise.resolve(scenario.after!(dom))
        .catch((err) => console.error('[harness] after() failed', err))
        .finally(ready);
    }, 150);
  } else {
    window.setTimeout(ready, 1500);
  }
  if (scenario.route === 'phone') return PwaApp;
  if (scenario.route === 'askbar') return AskBarApp;
  return scenario.route === 'settings' ? SettingsApp : CockpitApp;
}
