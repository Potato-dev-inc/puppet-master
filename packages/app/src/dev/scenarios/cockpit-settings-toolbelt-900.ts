import type { HarnessScenario } from '../types';

// Same deep link at 900px wide.
const s: HarnessScenario = {
  name: 'cockpit-settings-toolbelt-900',
  route: 'cockpit',
  window: { width: 900, height: 640 },
  after: async (dom) => {
    dom.key('k', { ctrl: true });
    const input = (await dom.waitFor('input')) as HTMLInputElement | null;
    if (input) {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'toolbelt');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    await dom.sleep(300);
    dom.key('Enter');
    await dom.sleep(900);
  },
};
export default s;
