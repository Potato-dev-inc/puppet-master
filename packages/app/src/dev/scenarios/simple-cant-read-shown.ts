import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// A question Simple cannot word: the raw screen stays behind "Show the screen" until pressed (1440 wide).
export default simpleScenario({
  name: 'simple-cant-read-shown',
  window: { width: 1440, height: 900 },
  simple: { panes: [{ id: 'pane-quiz', status: 'waiting_input', repo: QUIZ_REPO, html: QUIZ_HTML, prompt: null }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.waitFor('[data-testid="cant-read"]');
    await dom.clickText('button', 'Show the screen');
    await dom.waitFor('.sx-screen');
    await dom.sleep(500);
  },
});
