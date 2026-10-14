import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-project-working-900 at 900x700.
export default simpleScenario({
  name: 'simple-project-working-900',
  window: { width: 900, height: 700 },
  simple: { panes: [{ id: 'pane-quiz', status: 'running', repo: QUIZ_REPO, html: null, filesChanged: 0 }] },
  select: 'pane-quiz',
});
