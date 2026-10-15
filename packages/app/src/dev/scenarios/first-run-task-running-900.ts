import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 4, the task is running (900 wide).
export default firstRunScenario({
  name: 'first-run-task-running-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'task', project: 'D:/code/shop', agent: 'claude', task: 'running' },
  after: async (dom) => {
    await dom.clickText('button', 'Run the task');
    await dom.waitFor('.fr-working');
    await dom.sleep(300);
  },
});
