import type { HarnessScenario } from '../types';

// Row search across sections: "key" finds rows in Keys, Phone and more, grouped under their section.
const s: HarnessScenario = {
  name: 'settings-search',
  route: 'settings',
  settingsTab: 'general',
  after: async (dom) => {
    const input = (await dom.waitFor('input[aria-label="Search settings"]')) as HTMLInputElement | null;
    if (!input) return;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'key');
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await dom.sleep(300);
  },
};
export default s;
