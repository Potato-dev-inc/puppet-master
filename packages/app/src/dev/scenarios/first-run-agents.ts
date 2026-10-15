import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 2, the doctor list inline, nothing tested yet.
export default firstRunScenario({
  name: 'first-run-agents',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'agents' },
});
