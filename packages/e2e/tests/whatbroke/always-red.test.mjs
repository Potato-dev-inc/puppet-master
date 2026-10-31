// What broke it: the tests are red even at the oldest save point, so no turn can be blamed.
import { sharedFolder, FIXTURES, paneInFolder, openSheetOnSelected, declareTest, findIt } from './_area.mjs';

export const meta = {
  id: 'whatbroke.always-red',
  title: 'When the oldest save point already fails, the sheet says it cannot look further back',
  area: 'whatbroke',
  tags: ['whatbroke'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await t.step('The first agent turn already creates the file that fails the tests');
  const cwd = await sharedFolder(t, 'red', FIXTURES.red);
  await paneInFolder(t, cwd);

  await t.step('Open What broke the tests? and search');
  await openSheetOnSelected(t);
  await declareTest(t, 'node check.mjs');
  await findIt(t);

  await t.step('The sheet says the tests were already failing at the oldest save point');
  await t.expect.text('already failing at the oldest save point', { timeout: 90000 });
  await t.expect.noText('Turn 1 by');
  await t.expect.notVisible('exact=Undo only this turn');
}
