import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 6, Add Puppet Master to your agents.
export default firstRunScenario({
  name: 'first-run-pack',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'pack' },
});
