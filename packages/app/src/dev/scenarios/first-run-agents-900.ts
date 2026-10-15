import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 2, the doctor list inline, nothing tested yet (900 wide).
export default firstRunScenario({
  name: 'first-run-agents-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'agents' },
});
