import type { HarnessScenario } from '../types';

// Settings > Agents > Preload with a 5 second idle timeout in the stored config: the field must show 5 seconds.
const s: HarnessScenario = { name: 'settings-preload-seconds', route: 'settings', settingsTab: 'agents', standbyIdleSecs: 5 };
export default s;
