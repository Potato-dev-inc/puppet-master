import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';
import { PRESET } from '../ship-fixtures';

// Project > Copies: setup commands, files copied in (never committed), the start command, a conductor.json to import.
const s: HarnessScenario = {
  name: 'project-copies',
  route: 'cockpit',
  select: PANE_CLAUDE,
  ship: { preset: { ...PRESET, conductor: { setup: ['npm ci', 'npm run build'], run: 'npm run dev', unsupported: null } } },
  after: async (dom) => {
    await dom.clickText('.ck-side-foot button', 'Project');
    await dom.waitFor('.ck-memory-file');
    await dom.clickText('[role="tab"]', 'Copies');
    await dom.waitFor('.ck-copies');
    await dom.sleep(600);
  },
};
export default s;
