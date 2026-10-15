import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 1, "I build with AI" chosen: Simple mode note (900 wide).
export default firstRunScenario({
  name: 'first-run-welcome-chosen-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'welcome' },
  after: async (dom) => {
    await dom.clickText('button[role="radio"]', 'I build with AI');
    await dom.waitFor('[data-testid="look-note"]');
    await dom.sleep(300);
  },
});
