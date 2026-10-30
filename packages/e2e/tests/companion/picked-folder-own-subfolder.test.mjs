// "Change folder" with the Desktop picked: the home is a folder of its own INSIDE it, never the Desktop itself.
import fs from 'node:fs';
import path from 'node:path';
import { noCompanions, openSetup, scrollSheetDown, stubFolderPicker, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.picked-folder-own-subfolder',
  title: 'Picking the Desktop makes the companion a folder of its own inside it',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const desktop = path.join(t.app.dirs.home, 'Desktop');
  fs.mkdirSync(desktop, { recursive: true });
  await stubFolderPicker(t, desktop); // the system folder picker cannot be clicked by a test
  await openSetup(t);
  await t.click('exact=Nimbus');

  await t.step('Change the folder to the Desktop');
  await scrollSheetDown(t);
  await t.click('exact=Change folder');

  await t.step('The sheet says the companion lives in a folder called mochi inside it');
  await t.expect.text(/Desktop.mochi/);

  await t.step('Wake the companion up');
  await t.click('exact=Wake up Mochi');
  await t.waitFor('exact=Sleep', { timeout: 20000 });

  await t.step('The notes are in Desktop/mochi; the Desktop itself got nothing');
  const [c] = await companionList(t);
  t.expect.equal(path.normalize(c.folder).toLowerCase(), path.join(desktop, 'mochi').toLowerCase());
  t.expect.ok(fs.existsSync(path.join(desktop, 'mochi', 'SOUL.md')), 'SOUL.md is in the own folder');
  t.expect.ok(!fs.existsSync(path.join(desktop, 'SOUL.md')), 'nothing was written onto the Desktop');
  t.expect.ok(!fs.existsSync(path.join(desktop, '.puppet-master')), 'no marker on the Desktop');
});
