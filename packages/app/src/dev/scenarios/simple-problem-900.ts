import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-problem-900 at 900x700.
export default simpleScenario({
  name: 'simple-problem-900',
  window: { width: 900, height: 700 },
  simple: { panes: [{ id: 'pane-quiz', status: 'error', repo: QUIZ_REPO, html: QUIZ_HTML, errorCode: 'AGENT_EXITED' }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.waitFor('[data-testid="problem-card"]');
    await dom.sleep(500);
  },
});
