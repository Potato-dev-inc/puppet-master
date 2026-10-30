// Branch from here: Cancel closes the sheet and no copy or agent was made.
import fs from 'node:fs';
import path from 'node:path';
import { oneTurnFolder, openBranchSheet } from './_area.mjs';

export const meta = {
  id: 'branch.cancel-makes-nothing',
  title: 'Cancel closes the Branch sheet and no copy is made',
  area: 'branch',
  tags: ['branch'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  const cwd = await oneTurnFolder(t);
  const panes = (await t.panes()).length;

  await t.step('Open the sheet and type a task');
  await openBranchSheet(t, 'add notes', 0);
  await t.fill('css=textarea', 'a task that will not be started');

  await t.step('Press Cancel');
  await t.click('exact=Cancel');
  await t.expect.notVisible('Save point:');

  await t.step('No new agent, no copy folder');
  t.expect.equal((await t.panes()).length, panes);
  t.expect.ok(!fs.existsSync(path.join(cwd, '.puppet-master', 'worktrees')), 'no copy folder was made');
  await t.expect.noText('1 helper');
}
