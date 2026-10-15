import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 3, Recent with a refused folder (Documents).
export default firstRunScenario({
  name: 'first-run-project',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'project' },
});
