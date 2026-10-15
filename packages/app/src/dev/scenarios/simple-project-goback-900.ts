import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-project-goback-900 at 900x700.
export default simpleScenario({
  name: 'simple-project-goback-900',
  window: { width: 900, height: 700 },
  simple: { panes: [{ id: 'pane-quiz', status: 'idle', repo: QUIZ_REPO, html: QUIZ_HTML }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Go back');
    await dom.waitFor('.sx-versions .sx-row');
  },
});
