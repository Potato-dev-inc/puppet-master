import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 1, nothing chosen yet.
export default firstRunScenario({
  name: 'first-run-welcome',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'welcome' },
});
