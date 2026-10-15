import { firstRunScenario } from '../first-run-scenario';

// First-run welcome: step 3, a folder without Git: Set up Git here.
export default firstRunScenario({
  name: 'first-run-project-git',
  window: { width: 1440, height: 900 },
  firstRun: { step: 'project' },
  after: async (dom) => {
    await dom.clickText('.fr-row', 'landing-page');
    await dom.waitFor('[data-testid="git-step"] code');
    await dom.sleep(300);
  },
});
