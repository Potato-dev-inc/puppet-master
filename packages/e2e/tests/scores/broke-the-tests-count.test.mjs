// Scores: when What broke the tests blames an agent's turn, that agent's row says "Broke the tests 1 time".
import { basename } from 'node:path';
import { sharedFolder, FIXTURES, paneInFolder, openSheetOnSelected, declareTest, findIt, openSidebarPage } from './_area.mjs';

export const meta = {
  id: 'scores.broke-the-tests-count',
  title: 'After What broke the tests blames a turn, the Scores row of that agent says Broke the tests 1 time',
  area: 'scores',
  tags: ['scores', 'whatbroke'],
  timeout: 150000,
};

export async function run(t) {
  await t.step('Two agents finish turns in one folder; the second breaks the test command');
  const cwd = await sharedFolder(t, 'culprit', FIXTURES.culprit);
  await paneInFolder(t, cwd);

  await t.step('Find what broke the tests');
  await openSheetOnSelected(t);
  await declareTest(t, 'node check.mjs');
  await findIt(t);
  await t.expect.text('Turn 1 by Nimbus broke the tests', { timeout: 90000 });
  await t.click('exact=Close');

  await t.step('Open Scores for this folder: the Nimbus row carries the count, and it ranks nobody');
  await openSidebarPage(t, 'Scores');
  await t.expect.text(`This project: ${basename(cwd)}`);
  await t.expect.text('Broke the tests 1 time');
  await t.expect.noText('ahead');
}
