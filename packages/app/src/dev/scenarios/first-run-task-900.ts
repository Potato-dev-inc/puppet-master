import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 4, the suggested prompt before it runs (900 wide).
export default firstRunScenario({
  name: 'first-run-task-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'task', project: 'D:/code/shop', agent: 'claude' },
});
