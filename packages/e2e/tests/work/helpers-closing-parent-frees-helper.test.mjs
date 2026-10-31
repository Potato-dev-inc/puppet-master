// Helpers: closing the agent that started a helper does not end the helper. It becomes a top-level pane (no chip, no nesting).
import { calm, S, gitProject, orchestrator, startHelpers, spawnHelperCmd, helperChips, helperGroups, sidebarTitles, waitStatus, helpersOf } from './_area.mjs';

export const meta = {
  id: 'work.helpers-closing-parent-frees-helper',
  title: 'Closing the parent leaves the helper running as an ordinary top-level pane',
  area: 'work',
  tags: ['work', 'helpers', 'close'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const helperDir = gitProject(t, 'orphan', { 'README.md': 'x\n' });
  const orch = await orchestrator(t, {
    commands: [spawnHelperCmd(t, { cwd: helperDir, message: 'long errand' })],
    helperTurns: [{ on: 'long errand', steps: [S.think(40000), S.say('errand done')] }],
    holdMs: 1000,
  });
  await startHelpers(t, orch, 1);
  const [helper] = await helpersOf(t, orch);
  await t.expect.eventually(async () => (await helperChips(t)).join() === '1 helper', { message: 'the chip' });
  await waitStatus(t, orch, 'idle', 30000);

  await t.step('Close the agent (it is idle, so no question)');
  await t.page.rightClick('exact=start helpers', { within: 'css=.ck-side' });
  await t.click('exact=Close');
  await t.expect.eventually(async () => !(await t.panes()).some((p) => p.id === orch.id), { message: 'the agent pane is gone' });

  await t.step('The helper is still there, still working, and no longer nested');
  const left = (await t.panes()).find((p) => p.id === helper.id);
  t.expect.ok(left, 'the helper pane remains');
  t.expect.equal(left.status, 'running');
  await t.expect.eventually(async () => (await helperChips(t)).length === 0 && (await helperGroups(t)).length === 0, { message: 'no chip and no group any more' });
  await t.expect.eventually(async () => (await sidebarTitles(t)).length === 1, { message: 'one top-level row' });
}
