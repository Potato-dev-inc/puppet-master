import { simpleScenario } from '../simple-scenario';
import { ASK_NETWORK_PROMPT, QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-ask-medium at 1440x900.
export default simpleScenario({
  name: 'simple-ask-medium',
  window: { width: 1440, height: 900 },
  simple: { panes: [{ id: 'pane-quiz', status: 'waiting_input', repo: QUIZ_REPO, html: QUIZ_HTML, prompt: ASK_NETWORK_PROMPT }] },
  select: 'pane-quiz',
});
