import type { HarnessScenario } from '../types';

// The palette filtered to the commands that open something (their titles end in "…"): the features
// that used to hide in Settings are all there.
const s: HarnessScenario = {
  name: 'cockpit-palette-features',
  route: 'cockpit',
  after: async (dom) => {
    dom.key('k', { ctrl: true });
    await dom.waitFor('.ck-palette-input');
    const input = document.querySelector<HTMLInputElement>('.ck-palette-input');
    if (input) {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      set?.call(input, '…');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    await dom.sleep(600);
  },
};
export default s;
