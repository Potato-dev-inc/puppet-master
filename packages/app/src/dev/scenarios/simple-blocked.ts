import { simpleScenario } from '../simple-scenario';
import { BLOCKED_PROMPT, QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-blocked at 1440x900.
export default simpleScenario({
  name: 'simple-blocked',
  window: { width: 1440, height: 900 },
  simple: { panes: [{ id: 'pane-quiz', status: 'waiting_input', repo: QUIZ_REPO, html: QUIZ_HTML, prompt: BLOCKED_PROMPT }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.waitFor('[data-testid="blocked-card"]');
  },
});
