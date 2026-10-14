import type { HarnessScenario } from '../types';

// The separate Settings window (Individual mode) at 900px wide. It must look the same after the inline change.
const s: HarnessScenario = {
  name: 'settings-window-900',
  route: 'settings',
  settingsTab: 'agents',
  window: { width: 900, height: 640 },
};
export default s;
