// Closing the system folder picker without a choice leaves the default folder in place.
import { noCompanions, openSetup, scrollSheetDown, stubFolderPicker, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.folder-picker-cancelled',
  title: 'Closing the folder picker keeps the folder the companion would get',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await stubFolderPicker(t, null);
  await openSetup(t);
  await scrollSheetDown(t);
  await t.expect.text('A folder of its own, made for it');

  await t.step('Press Change folder and close the picker without choosing');
  await t.click('exact=Change folder');

  await t.step('The sheet still says the default folder, with no error');
  await t.expect.text('A folder of its own, made for it');
  t.expect.equal(await t.page.count('css=.ck-memory-err'), 0, 'no error');
});
