import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 3, a folder without Git: Set up Git here (900 wide).
export default firstRunScenario({
  name: 'first-run-project-git-900',
  window: { width: 900, height: 700 },
  firstRun: { step: 'project' },
  after: async (dom) => {
    await dom.clickText('.fr-row', 'landing-page');
    await dom.waitFor('[data-testid="git-step"] code');
    await dom.sleep(300);
  },
});
