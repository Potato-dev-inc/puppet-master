// Cancel and Escape close the setup sheet without making anything.
import { noCompanions, openSetup, scrollSheetDown, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.cancel-setup',
  title: 'Cancel and Escape close the setup sheet and make no companion',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);

  await t.step('Open the sheet and press Cancel');
  await openSetup(t);
  await scrollSheetDown(t);
  await t.click('exact=Cancel');
  await t.waitGone('Make your companion');

  await t.step('Open it again and press Escape');
  await openSetup(t);
  await t.press('Escape');
  await t.waitGone('Make your companion');

  await t.step('Nothing was made');
  t.expect.equal((await companionList(t)).length, 0, 'no companion');
  t.expect.equal((await t.panes()).length, 0, 'no pane');
  await t.expect.visible('Make a companion', { within: 'css=.ck-side' });
});
