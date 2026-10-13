import type { HarnessScenario } from '../types';

const s: HarnessScenario = {
  name: 'cockpit-palette-toolbelt',
  route: 'cockpit',
  after: async (dom) => {
    dom.key('k', { ctrl: true });
    const input = (await dom.waitFor('input')) as HTMLInputElement | null;
    if (input) {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'toolbelt');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    await dom.sleep(600);
  },
};
export default s;
