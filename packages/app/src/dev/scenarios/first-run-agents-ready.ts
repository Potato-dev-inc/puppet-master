import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 2 after Test on Claude Code: one agent is ready, Continue is on.
export default firstRunScenario({
  name: 'first-run-agents-ready',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'agents' },
  after: async (dom) => {
    await dom.clickText('button', 'Test');
    await dom.waitFor('.fr-status[data-ready="true"]');
    await dom.sleep(300);
  },
});
