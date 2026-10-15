import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 7, three tips (900 wide).
export default firstRunScenario({
  name: 'first-run-done-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'done' },
});
