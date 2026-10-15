import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// After a restore the toast carries an Undo that puts the "Before undo" checkpoint back.
// The screenshot is taken seconds after the scenario ends, so the toast's auto-dismiss timer
// (3-12 s) is parked here to keep it on screen.
const s: HarnessScenario = {
  name: 'review-fixes-restore-toast',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    const realSetTimeout = window.setTimeout.bind(window);
    (window as unknown as { setTimeout: unknown }).setTimeout = (fn: TimerHandler, ms?: number, ...rest: unknown[]) =>
      ms !== undefined && ms >= 3000 && ms <= 12000 ? 0 : realSetTimeout(fn, ms, ...rest);
    await dom.clickText('[role="tab"]', 'History');
    await dom.waitFor('.ck-hist-item');
    document.querySelectorAll<HTMLElement>('.ck-hist-row .ck-btn')[4]?.click();
    await dom.waitFor('[role="alertdialog"]');
    await dom.clickText('button', 'Restore files');
    await dom.waitFor('.ck-toast-action');
    await dom.sleep(500);
  },
};
export default s;
