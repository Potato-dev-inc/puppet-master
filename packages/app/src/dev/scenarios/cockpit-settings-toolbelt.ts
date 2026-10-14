import type { HarnessScenario } from '../types';

// Deep link: palette > Toolbelt opens Settings on Bridge & MCP. Before: a separate window (nothing here).
// After: inline, the Bridge & MCP page, sidebar kept.
const s: HarnessScenario = {
  name: 'cockpit-settings-toolbelt',
  route: 'cockpit',
  after: async (dom) => {
    dom.key('k', { ctrl: true });
    const input = (await dom.waitFor('input')) as HTMLInputElement | null;
    if (input) {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'toolbelt');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    await dom.sleep(300);
    await dom.clickText('[role="option"]', 'Toolbelt');
    await dom.sleep(900);
  },
};
export default s;
