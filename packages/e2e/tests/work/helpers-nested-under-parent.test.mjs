// Helpers: an agent starts a helper agent from inside its own pane (through the app's bridge, like its tool would). The helper's
// pane is nested under the agent in the sidebar with a count; a pane started from outside any agent is not nested.
import { calm, S, projectDir, gitProject, orchestrator, startHelpers, spawnHelperCmd, helperChips, helperGroups, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.helpers-nested-under-parent',
  title: 'A helper started from inside an agent\'s pane is nested under it with a "1 helper" count; other panes are not',
  area: 'work',
  tags: ['work', 'helpers', 'smoke'],
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const helperDir = gitProject(t, 'helperdir', { 'README.md': 'helper folder\n' });
  const orch = await orchestrator(t, {
    commands: [spawnHelperCmd(t, { cwd: helperDir, message: 'research task' })],
    helperTurns: [{ on: 'research task', steps: [S.think(3000), S.write('found.txt', 'the helper found it\n'), S.say('research done')] }],
  });

  await t.step('Ask the agent to start a helper');
  await startHelpers(t, orch, 1);
  const cmd = t.fake.events().find((e) => e.ev === 'run' && String(e.command).includes('spawn-helper'));
  await t.expect.eventually(() => t.fake.events().find((e) => e.ev === 'run' && String(e.command).includes('spawn-helper')), { message: 'the command ran' });

  await t.step('The helper pane records the agent as its parent');
  const panes = await t.panes();
  const helper = panes.find((p) => p.parent_pane_id === orch.id);
  t.expect.ok(helper, 'a pane has the agent as its parent');
  t.expect.equal(helper.cwd.replace(/\\/g, '/'), helperDir.replace(/\\/g, '/'));
  t.expect.ok(!panes.find((p) => p.id === orch.id).parent_pane_id, 'the agent itself has no parent');

  await t.step('The sidebar nests the helper under the agent behind a "1 helper" chip');
  await t.expect.eventually(async () => (await helperChips(t)).join() === '1 helper', { message: async () => `chips: ${JSON.stringify(await helperChips(t))}` });
  t.expect.equal((await helperGroups(t)).length, 1);
  t.expect.includes((await helperGroups(t))[0], 'start helpers');

  await t.step('The helper did its work in its own folder');
  await t.expect.eventually(() => fs.existsSync(path.join(helperDir, 'found.txt')), { timeout: 30000, message: 'found.txt in the helper folder' });

  await t.step('A pane started from outside any agent is top level: still only one chip');
  await t.spawnPane('nimbus', { cwd: projectDir(t, 'plain') });
  t.expect.equal((await helperChips(t)).join(), '1 helper');
  t.expect.equal((await t.panes()).filter((p) => p.parent_pane_id).length, 1);
  void cmd;
}
