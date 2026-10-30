// With no companion the sidebar has one quiet row that asks the person to make one; it opens the setup sheet.
import { noCompanions, openSetup, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.make-row-empty',
  title: 'With no companion the sidebar offers "Make a companion" and it opens the setup sheet',
  area: 'companion',
  tags: ['smoke'],
};

export const run = guarded(async (t) => {
  await noCompanions(t);

  await t.step('The sidebar has one row that offers to make a companion');
  await t.expect.visible('Make a companion', { within: 'css=.ck-side' });
  await t.expect.count('css=.ck-companion-row', 1);
  t.expect.equal(await t.page.count('css=.ck-companion-row:not(.ck-companion-row--empty)'), 0, 'no companion row yet');

  await t.step('Click it: the setup sheet opens');
  await openSetup(t);
  await t.expect.text('Your own helper that is always on.');
  await t.expect.text('Wake up Mochi');
});
