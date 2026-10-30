// Branch from here: the copy holds the files of the chosen save point, not the work done after it.
import fs from 'node:fs';
import { twoTurnFolder, openBranchSheet, copyPane } from './_area.mjs';

export const meta = {
  id: 'branch.earlier-point-lacks-later-work',
  title: 'A copy made from an earlier save point does not contain later work',
  area: 'branch',
  tags: ['branch'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await t.step('Two agents finished turns in order: notes.txt, then readme.txt');
  await twoTurnFolder(t);

  await t.step('On the second agent open History and branch from its "Before turn 1" row');
  await openBranchSheet(t, 'add readme', 1);
  await t.expect.eventually(async () => /Save point: Before turn 1/.test(await t.page.text()), { message: 'the sheet names the Before save point' });
  await t.fill('css=textarea', 'start over from here');
  await t.click('exact=Start Nimbus');

  await t.step('The copy has notes.txt (done before) but not readme.txt (done by this turn)');
  const copy = await copyPane(t);
  const files = fs.readdirSync(copy.cwd);
  t.expect.includes(files, 'notes.txt');
  t.expect.ok(!files.includes('readme.txt'), `the copy must not hold later work, it has ${JSON.stringify(files)}`);
}
