import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 3, Recent with a refused folder (Documents) (900 wide).
export default firstRunScenario({
  name: 'first-run-project-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'project' },
});
