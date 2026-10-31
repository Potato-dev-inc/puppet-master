// What broke it: Stop ends a long search at once and leaves a "Stopped" answer.
import { sharedFolder, FIXTURES, paneInFolder, openSheetOnSelected, declareTest, writeFiles, findIt } from './_area.mjs';

export const meta = {
  id: 'whatbroke.stop-ends-search',
  title: 'Stop ends a running search and the sheet says it was stopped',
  area: 'whatbroke',
  tags: ['whatbroke'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  const cwd = await sharedFolder(t, 'green', FIXTURES.green);
  await paneInFolder(t, cwd);

  await t.step('Declare a test command that takes a long time');
  // The folder's own new file, written before the search: the copy is made from save points, so the "now" save point holds it.
  writeFiles(cwd, { 'slow.mjs': 'setTimeout(() => {}, 120000);\n' });
  await openSheetOnSelected(t);
  await declareTest(t, 'node slow.mjs');
  await findIt(t);

  await t.step('The search is running: the sheet shows its progress and only Stop');
  await t.expect.text('Trying save point', { timeout: 60000 });
  await t.expect.visible('exact=Stop');

  await t.step('Press Stop');
  await t.click('exact=Stop');
  await t.expect.text('Stopped.', { timeout: 30000 });
  await t.expect.notVisible('exact=Stop');
  await t.expect.visible('exact=Close');
}
