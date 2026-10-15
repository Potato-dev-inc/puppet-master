import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-make-see at 1440x900.
export default simpleScenario({
  name: 'simple-make-see',
  window: { width: 1440, height: 900 },
  simple: { panes: [] },
  after: async (dom) => {
    await dom.clickText('.sx-make-btn', 'Make something');
    await dom.clickText('.sx-choice', 'A small tool');
    await dom.clickText('.sx-choice', 'A quiz with 10 questions');
    await dom.clickText('.sx-choice', 'My students or customers');
    await dom.clickText('.sx-choice', 'Bright and fun');
    await dom.waitFor('.sx-textarea');
    await dom.click('.sx-rules summary');
  },
});
