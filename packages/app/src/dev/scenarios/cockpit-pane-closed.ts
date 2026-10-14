import { emit } from '@tauri-apps/api/event';
import type { HarnessScenario } from '../types';
import { EXAMPLE_PANES, PANE_OPENCODE } from '../fixtures';

// The selected pane closes (as after Merge or Discard). A MutationObserver records whether the
// "Opening pane" screen is ever drawn in the next 2.2 s (the old grace period was 1.5 s) and the
// result is stamped on the page for the screenshot. Dev harness only.
const panes = [...EXAMPLE_PANES];

const s: HarnessScenario = {
  name: 'cockpit-pane-closed',
  route: 'cockpit',
  select: PANE_OPENCODE,
  panes,
  after: async (dom) => {
    await dom.sleep(800); // the pane is open and selected
    let sawOpening = false;
    const observer = new MutationObserver(() => {
      if (document.body.innerText.includes('Opening pane')) sawOpening = true;
    });
    observer.observe(document.body, { childList: true, subtree: true, characterData: true });
    panes.splice(
      panes.findIndex((p) => p.id === PANE_OPENCODE),
      1,
    );
    await emit('pty://panes-changed', {});
    await dom.sleep(2200);
    observer.disconnect();
    const stamp = document.createElement('div');
    stamp.textContent = `"Opening pane" screen shown after the pane closed: ${sawOpening ? 'YES (flash)' : 'no'}`;
    stamp.style.cssText =
      'position:fixed;left:260px;bottom:12px;padding:6px 10px;font:12px monospace;background:#fff;color:#000;border-radius:6px;z-index:9999';
    document.body.appendChild(stamp);
  },
};
export default s;
