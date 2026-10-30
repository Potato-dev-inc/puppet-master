// Branch from here: the Start button stays off until there is a task with words in it.
import { oneTurnFolder, openBranchSheet, isDisabled } from './_area.mjs';

export const meta = {
  id: 'branch.start-needs-a-task',
  title: 'Start stays off for an empty or blank task and turns on once there are words',
  area: 'branch',
  tags: ['branch'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await oneTurnFolder(t);
  await openBranchSheet(t, 'add notes', 0);

  await t.step('With nothing typed, Start Nimbus is off');
  t.expect.ok(await isDisabled(t, 'exact=Start Nimbus'), 'off with an empty task');

  await t.step('A task of only spaces keeps it off');
  await t.fill('css=textarea', '     ');
  t.expect.ok(await isDisabled(t, 'exact=Start Nimbus'), 'off with a blank task');

  await t.step('Real words turn it on');
  await t.fill('css=textarea', 'try the other approach');
  await t.expect.eventually(async () => !(await isDisabled(t, 'exact=Start Nimbus')), { message: 'Start Nimbus turns on' });

  await t.step('Delete the words again: off again');
  await t.fill('css=textarea', '');
  await t.expect.eventually(async () => isDisabled(t, 'exact=Start Nimbus'), { message: 'Start Nimbus turns off again' });
}
