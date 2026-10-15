import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-cant-read-900 at 900x700.
export default simpleScenario({
  name: 'simple-cant-read-900',
  window: { width: 900, height: 700 },
  simple: { panes: [{ id: 'pane-quiz', status: 'waiting_input', repo: QUIZ_REPO, html: QUIZ_HTML, prompt: null }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.waitFor('[data-testid="cant-read"]');
    await dom.sleep(500);
  },
});
