// Helpers: a helper that starts a helper of its own. The sidebar nests two levels and the top chip counts every helper below it.
import { calm, S, nimbus, gitProject, spawnHelperCmd, helperChips, helperGroups, helpersOf, waitHelpers, waitStatus } from './_area.mjs';

export const meta = {
  id: 'work.helpers-two-levels-deep',
  title: "A helper's own helper nests two levels deep and the top chip counts both (2 helpers)",
  area: 'work',
  tags: ['work', 'helpers'],
  edge: true,
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const midDir = gitProject(t, 'middle', { 'README.md': 'mid\n' });
  const leafDir = gitProject(t, 'leaf', { 'README.md': 'leaf\n' });

  t.fake.script(nimbus([{ on: 'start helpers', steps: [S.think(500), S.run(spawnHelperCmd(t, { cwd: midDir, message: 'plan subtasks' })), S.think(20000), S.say('top done')] }]));
  const top = await t.spawnPane('nimbus', { cwd: t.project('top') });
  t.fake.script(nimbus([
    { on: 'plan subtasks', steps: [S.think(500), S.run(spawnHelperCmd(t, { cwd: leafDir, message: 'do leaf work' })), S.think(15000), S.say('plan done')] },
    { on: 'do leaf work', steps: [S.think(500), S.write('leaf.txt', 'leaf was here\n'), S.say('leaf done')] },
  ]));

  await t.step('The agent starts a helper, and that helper starts one of its own');
  await top.write('start helpers');
  await waitHelpers(t, top, 1);
  const [mid] = await helpersOf(t, top);
  await waitHelpers(t, { id: mid.id }, 1);
  const [leaf] = await helpersOf(t, { id: mid.id });
  t.expect.ok(leaf, 'the second-level helper has the first helper as its parent');
  t.expect.ok(!(await t.panes()).find((p) => p.id === leaf.id).parent_pane_id?.startsWith(top.id), 'the leaf is not recorded under the top agent directly');

  await t.step('The top row shows "2 helpers" and a nested group inside the first helper shows "1 helper"');
  await t.expect.eventually(async () => (await helperChips(t)).sort().join() === '1 helper,2 helpers', { message: async () => `chips: ${JSON.stringify(await helperChips(t))}` });
  t.expect.equal((await helperGroups(t)).length, 2);

  await t.step('The leaf did its work');
  await t.expect.eventually(async () => (await t.panes()).find((p) => p.id === leaf.id)?.status === 'idle', { timeout: 40000, message: 'the leaf is idle' });
  void waitStatus;
}
