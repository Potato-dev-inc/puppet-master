import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-project-conversation at 1440x900.
export default simpleScenario({
  name: 'simple-project-conversation',
  window: { width: 1440, height: 900 },
  simple: { panes: [{ id: 'pane-quiz', status: 'idle', repo: QUIZ_REPO, html: QUIZ_HTML }] },
  select: 'pane-quiz',
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Conversation');
    await dom.waitFor('.sx-msg');
  },
});
