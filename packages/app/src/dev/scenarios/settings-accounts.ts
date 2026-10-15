import type { HarnessScenario } from '../types';
import { EXAMPLE_ACCOUNTS } from '../fixtures';

const s: HarnessScenario = { name: 'settings-accounts', route: 'settings', settingsTab: 'agents', accounts: EXAMPLE_ACCOUNTS };
export default s;
