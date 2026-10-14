import { simpleScenario } from '../simple-scenario';
import { QUIZ_HTML, QUIZ_REPO } from '../simple-fixtures';

// Generated for the evidence set: simple-project-working at 1440x900.
export default simpleScenario({
  name: 'simple-project-working',
  window: { width: 1440, height: 900 },
  simple: { panes: [{ id: 'pane-quiz', status: 'running', repo: QUIZ_REPO, html: null, filesChanged: 0 }] },
  select: 'pane-quiz',
});
