import { simpleScenario } from '../simple-scenario';

// Simple mode with no AI ready: "Set up an AI helper" opens Set up agents inside Simple mode (1440 wide).
export default simpleScenario({
  name: 'simple-helpers',
  window: { width: 1440, height: 900 },
  simple: { panes: [], noAgent: true },
  after: async (dom) => {
    await dom.clickText('.sx-make-btn', 'Make something');
    await dom.waitFor('[data-testid="no-agent"]');
    await dom.clickText('button', 'Set up an AI helper');
    await dom.waitFor('.ck-doctor-row');
    await dom.sleep(300);
  },
});
