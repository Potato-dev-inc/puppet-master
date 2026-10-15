import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 2 after Test on Claude Code: one agent is ready, Continue is on (900 wide).
export default firstRunScenario({
  name: 'first-run-agents-ready-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'agents' },
  after: async (dom) => {
    await dom.clickText('button', 'Test');
    await dom.waitFor('.fr-status[data-ready="true"]');
    await dom.sleep(300);
  },
});
