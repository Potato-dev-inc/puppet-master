// Escape, an empty name and an unchanged name all leave the companion as it was.
import { noCompanions, makeCompanion, openCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.rename-cancelled',
  title: "Escape or an empty name leaves the companion's name alone",
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);

  await t.step('Start typing a new name and press Escape');
  await t.click('exact=Mochi', { within: 'css=.ck-companion-head' });
  await t.page.fill('aria=Companion name', 'Nope');
  await t.press('Escape');
  await t.expect.visible('exact=Mochi', { within: 'css=.ck-companion-head' });

  await t.step('Clear the name and press Enter: an empty name is not saved');
  await t.click('exact=Mochi', { within: 'css=.ck-companion-head' });
  await t.page.fill('aria=Companion name', '');
  await t.press('Enter');
  await t.expect.visible('exact=Mochi', { within: 'css=.ck-companion-head' });

  await t.step('A name of only spaces is not saved either');
  await t.click('exact=Mochi', { within: 'css=.ck-companion-head' });
  await t.page.fill('aria=Companion name', '   ');
  await t.press('Enter');
  await t.expect.visible('exact=Mochi', { within: 'css=.ck-companion-head' });
  t.expect.equal((await companionList(t))[0].name, 'Mochi');
});
