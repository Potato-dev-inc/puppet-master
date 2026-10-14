import { simpleScenario } from '../simple-scenario';
import { ASK_PROMPT, ORDERS_REPO, TIMER_REPO, QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-home-900 at 900x700.
export default simpleScenario({
  name: 'simple-home-900',
  window: { width: 900, height: 700 },
  simple: { panes: [
    { id: 'pane-quiz', status: 'running', repo: QUIZ_REPO },
    { id: 'pane-orders', status: 'waiting_input', repo: ORDERS_REPO, prompt: ASK_PROMPT },
    { id: 'pane-timer', status: 'idle', repo: TIMER_REPO },
  ] },
});
