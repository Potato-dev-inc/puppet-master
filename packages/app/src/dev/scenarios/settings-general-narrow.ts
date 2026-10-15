import type { HarnessScenario } from '../types';

// Settings > General at the smallest window the app allows (720 wide) with a realistic long project path.
const s: HarnessScenario = {
  name: 'settings-general-narrow',
  route: 'settings',
  settingsTab: 'general',
  window: { width: 720, height: 560 },
};
export default s;
