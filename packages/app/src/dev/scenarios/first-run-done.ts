import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 7, three tips.
export default firstRunScenario({
  name: 'first-run-done',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'done' },
});
