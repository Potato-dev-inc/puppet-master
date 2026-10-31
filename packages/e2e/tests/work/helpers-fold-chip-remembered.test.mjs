// Helpers: the "N helpers" chip folds the helpers away, and the fold is remembered after the page reloads.
import { calm, S, gitProject, orchestrator, startHelpers, spawnHelperCmd, helperChips, helperGroups, sidebarTitles } from './_area.mjs';

export const meta = {
  id: 'work.helpers-fold-chip-remembered',
  title: 'Folding "1 helper" hides the helper row, unfolding brings it back, and the fold survives a reload',
  area: 'work',
  tags: ['work', 'helpers'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const helperDir = gitProject(t, 'folddir', { 'README.md': 'x\n' });
  const orch = await orchestrator(t, {
    commands: [spawnHelperCmd(t, { cwd: helperDir, message: 'sidequest' })],
    helperTurns: [{ on: 'sidequest', steps: [S.think(500), S.say('sidequest done')] }],
    holdMs: 3000,
  });
  await startHelpers(t, orch, 1);
  await t.expect.eventually(async () => (await helperChips(t)).join() === '1 helper', { message: 'the chip' });

  await t.step('The helper has its own row under the agent');
  await t.expect.eventually(async () => (await sidebarTitles(t)).length === 2, { message: async () => `rows: ${JSON.stringify(await sidebarTitles(t))}` });
  const before = await sidebarTitles(t);

  await t.step('Fold: the chip says so and the helper row is gone');
  await t.click('exact=1 helper', { within: 'css=.ck-side' });
  await t.expect.eventually(async () => (await sidebarTitles(t)).length === 1, { message: async () => `rows after fold: ${JSON.stringify(await sidebarTitles(t))}` });
  t.expect.equal((await sidebarTitles(t))[0], before[0], 'the agent row stays');

  await t.step('Reload the page: still folded');
  await t.page.reload();
  await t.expect.text('Inbox');
  await t.expect.eventually(async () => (await helperChips(t)).join() === '1 helper' && (await sidebarTitles(t)).length === 1, { message: 'folded after reload' });

  await t.step('Unfold: the helper is back');
  await t.click('exact=1 helper', { within: 'css=.ck-side' });
  await t.expect.eventually(async () => (await sidebarTitles(t)).length === 2, { message: 'two rows again' });
  t.expect.equal((await helperGroups(t)).length, 1);
}
