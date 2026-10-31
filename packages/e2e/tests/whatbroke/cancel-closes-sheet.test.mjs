// What broke it: Cancel closes the sheet and nothing was saved or started.
import { gitProject, CHECK_MJS, paneWithFirstSavePoint, openSheetOnSelected, WB_SHEET } from './_area.mjs';

export const meta = {
  id: 'whatbroke.cancel-closes-sheet',
  title: 'Cancel closes the What broke the tests sheet without saving a test command',
  area: 'whatbroke',
  tags: ['whatbroke'],
  edge: true,
};

export async function run(t) {
  const cwd = gitProject(t, 'cancel', { 'check.mjs': CHECK_MJS });
  await paneWithFirstSavePoint(t, cwd);

  await t.step('Open the sheet and type a command without saving it');
  await openSheetOnSelected(t);
  await t.fill(`${WB_SHEET} input`, 'node check.mjs');

  await t.step('Press Cancel');
  await t.click('exact=Cancel');
  await t.expect.notVisible('Test command');

  await t.step('Open it again: no command was saved');
  await t.click('exact=What broke the tests?');
  await t.waitFor('Test command');
  await t.expect.text('None yet');
  await t.expect.value(`${WB_SHEET} input`, '');
}
