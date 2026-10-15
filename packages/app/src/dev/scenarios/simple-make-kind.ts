import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-make-kind at 1440x900.
export default simpleScenario({
  name: 'simple-make-kind',
  window: { width: 1440, height: 900 },
  simple: { panes: [] },
  after: async (dom) => {
    await dom.clickText('.sx-make-btn', 'Make something');
    await dom.waitFor('.sx-choice');
  },
});
