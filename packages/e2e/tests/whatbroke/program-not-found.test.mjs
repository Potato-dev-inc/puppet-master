// What broke it: a test command whose program is not installed gives a plain "not found" result, not a blame.
import { sharedFolder, FIXTURES, paneInFolder, openSheetOnSelected, declareTest, findIt } from './_area.mjs';

export const meta = {
  id: 'whatbroke.program-not-found',
  title: 'A test command whose program is not on this PC is reported plainly and blames no turn',
  area: 'whatbroke',
  tags: ['whatbroke'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  const cwd = await sharedFolder(t, 'green', FIXTURES.green);
  await paneInFolder(t, cwd);

  await t.step('Declare a test command for a program that does not exist');
  await openSheetOnSelected(t);
  await declareTest(t, 'no-such-tool-e2e --run');
  await findIt(t);

  await t.step('The sheet says the program was not found and names no agent');
  await t.expect.text('was not found on this PC', { timeout: 60000 });
  await t.expect.noText('Turn 1 by');
  await t.expect.notVisible('exact=Undo only this turn');
}
