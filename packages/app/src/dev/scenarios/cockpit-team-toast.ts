import { emit } from '@tauri-apps/api/event';
import { MY_DRAFT, teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// The sending side: Leo's Claude Code just pushed feat/billing. The toast asks "Tell the team?",
// with the preview of the card Maya and Sam will see opened.
const s: HarnessScenario = {
  name: 'cockpit-team-toast',
  route: 'cockpit',
  team: teamState({ drafts: [MY_DRAFT], cards: [] }),
  after: async (dom) => {
    await dom.sleep(300);
    await emit('team://draft', MY_DRAFT);
    await dom.clickText('.ck-tell-toast button', 'Preview');
    await dom.sleep(200);
  },
};
export default s;
