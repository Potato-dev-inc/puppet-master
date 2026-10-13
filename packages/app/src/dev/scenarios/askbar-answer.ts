import type { HarnessScenario } from '../types';

const QUESTION = 'How do I share a cache between threads in Rust?';

const s: HarnessScenario = {
  name: 'askbar-answer',
  route: 'askbar',
  after: async (dom) => {
    const input = (await dom.waitFor('.askbar-input')) as HTMLInputElement | HTMLTextAreaElement | null;
    if (!input) return;
    // React ignores plain .value writes; go through the native setter so onChange fires.
    const proto = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(input, QUESTION);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await dom.sleep(100);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    await dom.waitFor('.askbar-code', 6000);
    await dom.sleep(400);
  },
};
export default s;
