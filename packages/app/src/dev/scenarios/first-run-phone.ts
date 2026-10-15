import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 5, pairing.
export default firstRunScenario({
  name: 'first-run-phone',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'phone' },
});
