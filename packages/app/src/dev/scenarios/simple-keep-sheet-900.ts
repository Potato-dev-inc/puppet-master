import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-keep-sheet-900 at 900x700.
export default simpleScenario({
  name: 'simple-keep-sheet-900',
  window: { width: 900, height: 700 },
  simple: { panes: [{ id: 'pane-quiz', status: 'idle', repo: QUIZ_REPO, html: QUIZ_HTML }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.clickText('.sx-bar button', 'Keep');
    await dom.sleep(300);
  },
});
