// What broke it: with no finished turn there is nothing to look back through, and Find it stays off.
import { gitProject, CHECK_MJS, paneWithFirstSavePoint, openSheetOnSelected, declareTest, isDisabled } from './_area.mjs';

export const meta = {
  id: 'whatbroke.too-few-save-points',
  title: 'With only one save point the sheet says so and Find it stays off',
  area: 'whatbroke',
  tags: ['whatbroke'],
  edge: true,
};

export async function run(t) {
  const cwd = gitProject(t, 'young', { 'check.mjs': CHECK_MJS });
  await paneWithFirstSavePoint(t, cwd);

  await t.step('Open What broke the tests? in a folder with no finished turn');
  await openSheetOnSelected(t);
  await t.expect.text('There is only one save point so far.');

  await t.step('Save a test command: Find it is still off');
  await declareTest(t, 'node check.mjs');
  await t.expect.text('node check.mjs');
  await t.expect.noText('None yet');
  t.expect.ok(await isDisabled(t, 'exact=Find it'), 'Find it should be disabled with one save point');
}
