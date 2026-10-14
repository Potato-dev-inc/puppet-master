import { GITHUB_CARDS, teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// "Merge when green" on an open pull request: what GitHub will do, and the button that asks for it.
const s: HarnessScenario = {
  name: 'cockpit-team-merge-sheet',
  route: 'cockpit',
  team: teamState({ cards: [GITHUB_CARDS[0]] }),
  after: async (dom) => {
    await dom.clickText('.ck-team-card button', 'Merge when green');
    await dom.waitFor('.ck-team-sheet .ck-sheet-actions');
    await dom.sleep(300);
  },
};
export default s;
