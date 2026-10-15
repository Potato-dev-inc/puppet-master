import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 4, the answer, the receipt and the two hints (900 wide).
export default firstRunScenario({
  name: 'first-run-task-done-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'task', project: 'D:/code/shop', agent: 'claude', task: 'done' },
  after: async (dom) => {
    await dom.clickText('button', 'Run the task');
    await dom.waitFor('[data-testid="task-done"]', 8000);
    await dom.sleep(300);
  },
});
