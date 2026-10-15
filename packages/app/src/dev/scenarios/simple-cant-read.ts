import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-cant-read at 1440x900.
export default simpleScenario({
  name: 'simple-cant-read',
  window: { width: 1440, height: 900 },
  simple: { panes: [{ id: 'pane-quiz', status: 'waiting_input', repo: QUIZ_REPO, html: QUIZ_HTML, prompt: null }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.waitFor('[data-testid="cant-read"]');
    await dom.sleep(500);
  },
});
