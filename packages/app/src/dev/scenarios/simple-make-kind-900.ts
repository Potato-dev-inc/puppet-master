import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-make-kind-900 at 900x700.
export default simpleScenario({
  name: 'simple-make-kind-900',
  window: { width: 900, height: 700 },
  simple: { panes: [] },
  after: async (dom) => {
    await dom.clickText('.sx-make-btn', 'Make something');
    await dom.waitFor('.sx-choice');
  },
});
