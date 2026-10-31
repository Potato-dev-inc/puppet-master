// Review desk: Throw away starts a 10 second countdown; Undo inside it keeps everything as it was.
import fs from 'node:fs';
import path from 'node:path';
import { gitProject, writingTurn, openDeskWithPane, rowSel, rowPosition } from './_area.mjs';

export const meta = {
  id: 'review.throw-away-undo-window',
  title: 'Throw away shows a 10 second Undo and pressing Z in time leaves the folder and the row alone',
  area: 'review',
  tags: ['review'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await t.step('An agent finishes a turn that adds a file');
  const cwd = gitProject(t, 'site', { 'README.md': 'site\n' });
  const pane = await writingTurn(t, cwd, 'add a page', [['src/page.txt', 'hello page']]);

  await t.step('Open the desk and select the row');
  await openDeskWithPane(t, pane);
  await t.click(rowSel(pane));

  await t.step('Press X: a countdown with Undo shows, and the row leaves the list');
  await t.press('x');
  await t.expect.text('Throwing away');
  await t.expect.text('Undo (Z)');
  await t.expect.eventually(async () => (await rowPosition(t, pane)) === -1, { message: 'the row is hidden while the countdown runs' });

  await t.step('Press Z before the time is up');
  await t.press('z');
  await t.expect.noText('Throwing away');

  await t.step('The row is back and the file is still in the folder');
  await t.expect.eventually(async () => (await rowPosition(t, pane)) >= 0, { message: 'the row is back' });
  t.expect.ok(fs.existsSync(path.join(cwd, 'src', 'page.txt')), 'src/page.txt is still there');
}
