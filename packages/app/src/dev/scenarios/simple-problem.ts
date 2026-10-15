import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-problem at 1440x900.
export default simpleScenario({
  name: 'simple-problem',
  window: { width: 1440, height: 900 },
  simple: { panes: [{ id: 'pane-quiz', status: 'error', repo: QUIZ_REPO, html: QUIZ_HTML, errorCode: 'AGENT_EXITED' }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.waitFor('[data-testid="problem-card"]');
    await dom.sleep(500);
  },
});
