import type { HarnessScenario } from '../types';
import { EXAMPLE_ACCOUNTS } from '../fixtures';

// Settings > Agents (the section the window opens on): Accounts, Keep agents ready, Agent definitions.
const s: HarnessScenario = { name: 'settings', route: 'settings', settingsTab: 'agents', accounts: EXAMPLE_ACCOUNTS };
export default s;
