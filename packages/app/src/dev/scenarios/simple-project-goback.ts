import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-project-goback at 1440x900.
export default simpleScenario({
  name: 'simple-project-goback',
  window: { width: 1440, height: 900 },
  simple: { panes: [{ id: 'pane-quiz', status: 'idle', repo: QUIZ_REPO, html: QUIZ_HTML }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Go back');
    await dom.waitFor('.sx-versions .sx-row');
  },
});
