import type { HarnessScenario } from '../types';

// Settings > Keys > OpenCode: switching to another key at a rate limit is an opt-in switch, off by default.
const s: HarnessScenario = { name: 'settings-keys-automation', route: 'settings', settingsTab: 'keys' };
export default s;
