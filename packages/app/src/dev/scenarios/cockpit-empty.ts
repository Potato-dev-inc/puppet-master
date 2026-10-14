import type { HarnessScenario } from '../types';

// No panes at all: the first thing a new user sees.
const s: HarnessScenario = { name: 'cockpit-empty', route: 'cockpit', panes: [] };
export default s;
