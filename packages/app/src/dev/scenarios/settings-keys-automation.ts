import type { HarnessScenario } from '../types';

// Key switching at a rate limit is an opt-in Labs feature, off by default. It lives in General > Labs; the deep link
// `opencode-automation` opens it there. (The OpenCode keys page keeps a link to it.)
const s: HarnessScenario = {
  name: 'settings-keys-automation',
  route: 'settings',
  settingsTab: 'opencode-automation',
  after: async (dom) => {
    await dom.sleep(500);
  },
};
export default s;
