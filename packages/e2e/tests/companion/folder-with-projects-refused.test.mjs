// A folder that already has projects in it (a .git folder) is refused in the sheet, in plain words; another folder works.
import fs from 'node:fs';
import path from 'node:path';
import { noCompanions, openSetup, scrollSheetDown, stubFolderPicker, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.folder-with-projects-refused',
  title: 'A folder with projects in it is refused in plain words and another folder works',
  area: 'companion',
  tags: ['security'],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  // crowded/mochi is the folder the companion would get, and it already holds a project (a git folder two levels down)
  const crowded = t.project('crowded');
  fs.mkdirSync(path.join(crowded, 'mochi', 'app', '.git'), { recursive: true });
  const free = t.project('free');
  await stubFolderPicker(t, crowded);
  await openSetup(t);
  await t.click('exact=Nimbus');

  await t.step('Pick the crowded folder and try to wake the companion');
  await scrollSheetDown(t);
  await t.click('exact=Change folder');
  await t.click('exact=Wake up Mochi');

  await t.step('The sheet refuses in plain words and offers the fix');
  await t.expect.text('Pick a folder of its own.');
  await t.expect.text('has other projects in it');
  await t.expect.noText('COMPANION_FOLDER_BROAD');
  t.expect.equal((await companionList(t)).length, 0, 'no companion was made');
  t.expect.ok(!fs.existsSync(path.join(crowded, 'mochi', 'SOUL.md')), 'nothing was written into the crowded folder');
  await t.expect.visible('Make your companion');

  await t.step('Pick a free folder instead');
  await stubFolderPicker(t, free);
  await t.click('exact=Change folder');
  await t.click('exact=Wake up Mochi');

  await t.step('Now it wakes up');
  await t.waitFor('exact=Sleep', { timeout: 20000 });
  const [c] = await companionList(t);
  t.expect.equal(path.normalize(c.folder).toLowerCase(), path.join(free, 'mochi').toLowerCase());
});
