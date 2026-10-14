import { emit } from '@tauri-apps/api/event';
import type { HarnessScenario } from '../types';

// The spawn screen of the cockpit while an agent starts: the log reads in plain words.
const s: HarnessScenario = {
  name: 'cockpit-spawn',
  route: 'cockpit',
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.sleep(400);
    await dom.clickText('[role="menuitem"]', 'Claude Code');
    await dom.sleep(400);
    for (const stage of ['claiming', 'spawning', 'attached'] as const) {
      await emit('pane://spawn-stage', { agent_id: 'claude', pane_id: 'pane-new-1', stage, warm: true });
      await dom.sleep(250);
    }
    await dom.sleep(500);
  },
};
export default s;
