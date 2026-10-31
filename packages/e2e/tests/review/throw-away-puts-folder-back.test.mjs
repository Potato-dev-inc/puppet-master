// Review desk: when the countdown ends, the folder goes back to how it was before the turn.
import fs from 'node:fs';
import path from 'node:path';
import { gitProject, writingTurn, openDeskWithPane, rowSel } from './_area.mjs';

export const meta = {
  id: 'review.throw-away-puts-folder-back',
  title: 'Throw away without Undo puts the folder back to how it was before the turn',
  area: 'review',
  tags: ['review'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await t.step('An agent finishes a turn that adds src/page.txt');
  const cwd = gitProject(t, 'site', { 'README.md': 'site\n' });
  const pane = await writingTurn(t, cwd, 'add a page', [['src/page.txt', 'hello page']]);
  t.expect.ok(fs.existsSync(path.join(cwd, 'src', 'page.txt')), 'the agent made the file');

  await t.step('Open the desk, select the row and press X');
  await openDeskWithPane(t, pane);
  await t.click(rowSel(pane));
  await t.press('x');
  await t.expect.text('Throwing away');

  await t.step('Wait for the countdown to run out');
  await t.expect.noText('Throwing away', { timeout: 30000 });

  await t.step('The file the turn made is gone and the files that were there before stay');
  await t.expect.eventually(() => !fs.existsSync(path.join(cwd, 'src', 'page.txt')), { timeout: 20000, message: 'src/page.txt should be removed' });
  t.expect.ok(fs.existsSync(path.join(cwd, 'README.md')), 'README.md stays');
}
