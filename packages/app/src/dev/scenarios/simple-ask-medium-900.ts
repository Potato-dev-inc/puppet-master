import { simpleScenario } from '../simple-scenario';
import { ASK_NETWORK_PROMPT, QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-ask-medium-900 at 900x700.
export default simpleScenario({
  name: 'simple-ask-medium-900',
  window: { width: 900, height: 700 },
  simple: { panes: [{ id: 'pane-quiz', status: 'waiting_input', repo: QUIZ_REPO, html: QUIZ_HTML, prompt: ASK_NETWORK_PROMPT }] },
  select: 'pane-quiz',
});
