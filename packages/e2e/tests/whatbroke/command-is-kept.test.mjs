// What broke it: the test command is saved for the folder and comes back the next time the sheet opens.
import { gitProject, CHECK_MJS, paneWithFirstSavePoint, openSheetOnSelected, declareTest } from './_area.mjs';

export const meta = {
  id: 'whatbroke.command-is-kept',
  title: 'The saved test command is shown again when the sheet is reopened',
  area: 'whatbroke',
  tags: ['whatbroke'],
};

export async function run(t) {
  const cwd = gitProject(t, 'kept', { 'check.mjs': CHECK_MJS });
  await paneWithFirstSavePoint(t, cwd);

  await t.step('Save the test command');
  await openSheetOnSelected(t);
  await declareTest(t, 'node check.mjs');
  await t.expect.text('node check.mjs');
  await t.expect.noText('None yet');

  await t.step('Close the sheet and open it again');
  await t.click('exact=Cancel');
  await t.expect.notVisible('Test command');
  await t.click('exact=What broke the tests?');
  await t.waitFor('Test command');

  await t.step('The command is still there; the box to type one is gone');
  await t.expect.text('node check.mjs');
  await t.expect.noText('None yet');
  await t.expect.visible('exact=Change');
}
