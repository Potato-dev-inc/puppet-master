import type { HarnessScenario } from '../types';

// Phone: the same Agents list scrolled to the grouped rows (Needs you / Working / Idle).
const s: HarnessScenario = {
  name: 'phone-agents-list',
  route: 'phone',
  window: { width: 390, height: 844 },
  phone: { digest: false },
  after: async (dom) => {
    await dom.waitFor('[data-question-id]');
    await dom.waitFor('.ph-group');
    await dom.sleep(600);
    const scroller = document.querySelector('.ph-scroll');
    if (scroller) scroller.scrollTop = 250;
    await dom.sleep(300);
  },
};
export default s;
