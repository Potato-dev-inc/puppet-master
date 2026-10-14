import type { HarnessScenario } from '../types';

// Settings > General > Save a diagnostic file, before "Review…" is pressed.
const s: HarnessScenario = {
  name: 'settings-diagnostic-file-row',
  route: 'settings',
  settingsTab: 'general',
  window: { width: 1100, height: 900 },
  after: async (dom) => {
    await dom.waitFor('[data-row="diagnostic-file"]');
    await dom.sleep(300);
    document.querySelector('[data-row="diagnostic-file"]')?.scrollIntoView({ block: 'center' });
  },
};
export default s;
