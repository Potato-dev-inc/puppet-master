import { GITHUB_CARDS, TEAM_CARDS, teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// GitHub facts in Inbox > Team: a pull request from someone outside the team (Merge when green), Sam's
// #141 merged into main, and a failed check on #142.
const s: HarnessScenario = {
  name: 'cockpit-team-github',
  route: 'cockpit',
  team: teamState({ cards: [...GITHUB_CARDS, TEAM_CARDS[1]] }),
};
export default s;
