import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 5, pairing (900 wide).
export default firstRunScenario({
  name: 'first-run-phone-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'phone' },
});
