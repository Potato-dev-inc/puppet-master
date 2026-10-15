import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 6, Add Puppet Master to your agents (900 wide).
export default firstRunScenario({
  name: 'first-run-pack-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'pack' },
});
