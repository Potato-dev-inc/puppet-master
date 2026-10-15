import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 1, "I build with AI" chosen: Simple mode note.
export default firstRunScenario({
  name: 'first-run-welcome-chosen',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'welcome' },
  after: async (dom) => {
    await dom.clickText('button[role="radio"]', 'I build with AI');
    await dom.waitFor('[data-testid="look-note"]');
    await dom.sleep(300);
  },
});
