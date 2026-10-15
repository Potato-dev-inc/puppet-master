import type { HarnessScenario } from '../types';

// A brand-new install: no agent was ever opened here. The Inbox welcomes the person and offers the one next step.
const s: HarnessScenario = { name: 'cockpit-welcome', route: 'cockpit', panes: [], firstRun: true };
export default s;
