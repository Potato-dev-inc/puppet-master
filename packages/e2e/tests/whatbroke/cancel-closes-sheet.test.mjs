// What broke it: Cancel closes the sheet and nothing was saved or started.
import { gitProject, CHECK_MJS, paneWithFirstSavePoint, openSheetOnSelected, WB_SHEET } from './_area.mjs';

export const meta = {
  id: 'whatbroke.cancel-closes-sheet',
  title: 'Cancel closes the What broke the tests sheet without saving a test command',
  area: 'whatbroke',
  tags: ['whatbroke'],
  edge: true,
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
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
