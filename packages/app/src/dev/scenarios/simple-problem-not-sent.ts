import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-problem-not-sent at 1440x900.
export default simpleScenario({
  name: 'simple-problem-not-sent',
  window: { width: 1440, height: 900 },
  simple: { panes: [{ id: 'pane-quiz', status: 'idle', repo: QUIZ_REPO, html: QUIZ_HTML, runFailedCode: 'PROMPT_NOT_SUBMITTED' }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.waitFor('[data-testid="problem-card"]');
  },
});
