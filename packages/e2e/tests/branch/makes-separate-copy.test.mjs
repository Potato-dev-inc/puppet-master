// Branch from here: a new agent starts in a separate copy; the original folder and chat are not touched.
import { oneTurnFolder, openBranchSheet, folderFiles, copyPane } from './_area.mjs';

export const meta = {
  id: 'branch.makes-separate-copy',
  title: 'Branching starts a new agent in a separate copy and leaves the original folder as it was',
  area: 'branch',
  tags: ['branch', 'smoke'],
  timeout: 90000,
};

export async function run(t) {
  const cwd = await oneTurnFolder(t);
  const before = folderFiles(cwd);
  const panesBefore = (await t.panes()).length;

  await t.step('Open Branch from here on the After row and say what the new agent should do');
  await openBranchSheet(t, 'add notes', 0);
  await t.fill('css=textarea', 'try a different layout');
  await t.expect.visible('exact=Start Nimbus');

  await t.step('Press Start Nimbus');
  await t.click('exact=Start Nimbus');

  await t.step('A new agent starts in a copy inside the project, not in the project itself');
  const copy = await copyPane(t);
  t.expect.equal((await t.panes()).length, panesBefore + 1);
  t.expect.ok(copy.cwd.replace(/\\/g, '/') !== cwd.replace(/\\/g, '/'), 'the copy has its own folder');

  await t.step('The copy has the files of the save point, and it was told where it comes from');
  const { default: fs } = await import('node:fs');
  t.expect.includes(fs.readdirSync(copy.cwd), 'notes.txt');
  t.expect.includes(fs.readdirSync(copy.cwd), 'README.md');
  await t.expect.eventually(async () => (await t.bridge.get(`/panes/${copy.id}/buffer`)).body.content.includes('Branch: you are starting a new line of work'), { message: 'the copy agent received the branch note' });

  await t.step('The sidebar shows the new agent under the original as a helper');
  await t.expect.text('1 helper');

  await t.step('The original folder has exactly the files it had');
  t.expect.equal(folderFiles(cwd), before);
}
