// Branch from here: a History row opens a sheet that says which save point it copies and who can do the work.
import { oneTurnFolder, openBranchSheet, isDisabled } from './_area.mjs';

export const meta = {
  id: 'branch.sheet-opens',
  title: 'Branch from here on a History row opens the sheet with the save point and the agents',
  area: 'branch',
  tags: ['branch', 'smoke'],
  timeout: 90000,
};

export async function run(t) {
  await t.step('An agent finishes one turn: History has a "Before" and an "After" save point');
  await oneTurnFolder(t);

  await t.step('Press "Branch from here" on the After row');
  await openBranchSheet(t, 'add notes', 0);

  await t.step('The sheet names the save point and says how many files that turn changed');
  await t.expect.text('Branch from here');
  await t.expect.eventually(async () => /Save point: After turn 1 · \d\d:\d\d · 1 file changed in that turn/.test(await t.page.text()), { message: 'the save point line' });

  await t.step('The agents are listed, the same agent first, and the start button waits for a task');
  await t.expect.text('What should it do?');
  await t.expect.visible('exact=Nimbus');
  await t.expect.visible('exact=Show what the agent gets');
  t.expect.ok(await isDisabled(t, 'exact=Start Nimbus'), 'Start Nimbus waits until you say what it should do');
  await t.expect.text('Your folder and this chat stay as they are.');
}
