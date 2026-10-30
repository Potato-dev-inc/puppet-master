// Helpers for the Scores tests (they sit on the shared kit of the What broke it area).
import { replyScript } from '../_helpers.mjs';
import { waitIdle, tauri } from '../whatbroke/_area.mjs';

export * from '../whatbroke/_area.mjs';

/** One finished turn of a pane: type the prompt, see the pane work, wait until it is idle again. */
export async function runTurn(t, pane, text) {
  await pane.write(text);
  await t.expect.eventually(async () => (await pane.info())?.status === 'running', { timeout: 10000, message: `expected the pane to start working on "${text}"` });
  await waitIdle(t, pane);
}

/** An agent that thinks for about a second, then answers. */
export const SLOW_REPLY = replyScript('finished', 1200);

/** Wait until the app's own Scores numbers count `n` turns for a folder (a turn is recorded a moment after it ends). */
export async function counted(t, cwd, n, { days = 90 } = {}) {
  let seen = 0;
  await t.expect.eventually(async () => {
    const report = await tauri(t, 'agent_scores', { project: cwd, days });
    seen = (report.rows || []).reduce((sum, r) => sum + (r.turns || 0), 0);
    return seen >= n;
  }, { timeout: 40000, message: () => `expected Scores to count ${n} turns for the folder, it counts ${seen}` });
}
