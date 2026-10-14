import type { HarnessScenario } from '../types';

// Settings > General > Save a diagnostic file: the row, then the preview list with one file opened.
const s: HarnessScenario = {
  name: 'settings-diagnostic-file',
  route: 'settings',
  settingsTab: 'general',
  window: { width: 1100, height: 900 },
  after: async (dom) => {
    await dom.waitFor('[data-row="diagnostic-file"]');
    await dom.clickText('button', 'Review');
    await dom.waitFor('.st-diag-list');
    await dom.clickText('.st-diag-item button', 'Show');
    await dom.sleep(300);
    document.querySelector('[data-row="diagnostic-file"]')?.scrollIntoView({ block: 'start' });
  },
};
export default s;
