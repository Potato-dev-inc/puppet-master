import type { HarnessScenario } from '../types';

// The "+" menu with an agent that has no pane of its own: its hint says who can use it.
const s: HarnessScenario = {
  name: 'cockpit-new-menu-headless',
  route: 'cockpit',
  headlessOnlyAgent: true,
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.sleep(800);
  },
};
export default s;
