import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 4, the suggested prompt before it runs.
export default firstRunScenario({
  name: 'first-run-task',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'task', project: 'D:/code/shop', agent: 'claude' },
});
