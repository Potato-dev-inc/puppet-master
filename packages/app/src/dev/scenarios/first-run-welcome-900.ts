import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 1, nothing chosen yet (900 wide).
export default firstRunScenario({
  name: 'first-run-welcome-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'welcome' },
});
