// Review desk: "Looks good" on a turn in your folder is a mark only. It takes the row away; Undo brings it back.
import fs from 'node:fs';
import path from 'node:path';
import { gitProject, writingTurn, openDeskWithPane, rowSel, rowPosition } from './_area.mjs';

export const meta = {
  id: 'review.looks-good-and-undo',
  title: 'Looks good takes a folder turn off the desk, changes nothing, and Undo puts it back',
  area: 'review',
  tags: ['review'],
  timeout: 90000,
};

export async function run(t) {
  await t.step('An agent finishes a turn that adds a file');
  const cwd = gitProject(t, 'site', { 'README.md': 'site\n' });
  const pane = await writingTurn(t, cwd, 'add a page', [['src/page.txt', 'hello page']]);

  await t.step('Open the desk, select the first row and press Y: Looks good');
  await openDeskWithPane(t, pane);
  await t.click(rowSel(pane));
  await t.press('y');

  await t.step('The row is gone and the desk says it was marked as looked at');
  await t.expect.text('Marked as looked at');
  await t.expect.eventually(async () => (await rowPosition(t, pane)) === -1, { message: 'the row of this turn is gone' });

  await t.step('Nothing in the folder changed');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'src', 'page.txt'), 'utf8'), 'hello page');

  await t.step('Press Z: the row comes back');
  await t.press('z');
  await t.expect.eventually(async () => (await rowPosition(t, pane)) >= 0, { message: 'the row is back' });
}
