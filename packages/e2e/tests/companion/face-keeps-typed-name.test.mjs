// Picking another face renames the companion to that face's name, until the person has typed a name of their own.
import { noCompanions, openSetup, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.face-keeps-typed-name',
  title: 'A new face brings its own name, but a name the person typed stays',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await openSetup(t);
  await t.expect.value('css=.ck-companion-input', 'Mochi');

  await t.step('Pick another face: its name comes with it');
  await t.click('css=button[title="Bit"]');
  await t.expect.value('css=.ck-companion-input', 'Bit');
  await t.expect.visible('exact=Wake up Bit');

  await t.step('Type a name of your own');
  await t.page.fill('css=.ck-companion-input', 'Pixel');
  await t.expect.visible('exact=Wake up Pixel');

  await t.step('Pick yet another face: the typed name stays');
  await t.click('css=button[title="Hoot"]');
  await t.expect.value('css=.ck-companion-input', 'Pixel');
  await t.expect.visible('exact=Wake up Pixel');
});
