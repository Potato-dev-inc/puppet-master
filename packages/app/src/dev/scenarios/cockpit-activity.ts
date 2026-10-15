import type { HarnessScenario } from '../types';

const s: HarnessScenario = { name: 'cockpit-activity', route: 'cockpit', after: async (dom) => { await dom.sleep(800); } };
export default s;
