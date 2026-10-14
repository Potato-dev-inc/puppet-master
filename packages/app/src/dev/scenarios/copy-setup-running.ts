import { emit } from '@tauri-apps/api/event';
import type { HarnessScenario } from '../types';

// While a separate copy is set up, the spawn screen says so before the agent starts.
const s: HarnessScenario = {
  name: 'copy-setup-running',
  route: 'cockpit',
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.sleep(400);
    await dom.click('[role="switch"]');
    await dom.clickText('[role="menuitem"]', 'Claude Code');
    await dom.sleep(400);
    await emit('pane://spawn-stage', { agent_id: 'claude', pane_id: null, stage: 'claiming', warm: false });
    await dom.sleep(200);
    await emit('pane://spawn-stage', { agent_id: 'claude', pane_id: null, stage: 'spawning', warm: false, message: 'Setting up the copy' });
    await dom.sleep(200);
    await emit('pane://spawn-stage', { agent_id: 'claude', pane_id: null, stage: 'spawning', warm: false, message: 'Ran npm ci; copied .env (never committed).' });
    await dom.sleep(500);
  },
};
export default s;
