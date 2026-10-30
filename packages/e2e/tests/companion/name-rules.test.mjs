// The name is 1 to 24 characters. An empty or blank name cannot be woken up; a longer one is cut at 24.
import { noCompanions, openSetup, scrollSheetDown, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.name-rules',
  title: 'An empty name cannot be woken up and a long name stops at 24 characters',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await openSetup(t);
  await t.click('exact=Nimbus');

  await t.step('Clear the name: the button cannot be pressed');
  await t.page.fill('css=.ck-companion-input', '');
  await scrollSheetDown(t);
  await t.expect.visible('exact=Wake up it');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('button')].find(b => b.textContent === 'Wake up it').disabled`), true, 'the button is disabled');

  await t.step('Only spaces is no name either');
  await t.page.fill('css=.ck-companion-input', '    ');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('button')].find(b => b.textContent.startsWith('Wake up')).disabled`), true, 'still disabled');

  await t.step('Type a name of 40 characters: only 24 are kept');
  await t.page.fill('css=.ck-companion-input', 'abcdefghijklmnopqrstuvwxyz0123456789ABCD');
  await t.expect.value('css=.ck-companion-input', 'abcdefghijklmnopqrstuvwx');
  await t.expect.visible('exact=Wake up abcdefghijklmnopqrstuvwx');

  await t.step('A real name makes the button work again');
  await t.page.fill('css=.ck-companion-input', 'Pip');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('button')].find(b => b.textContent === 'Wake up Pip').disabled`), false, 'enabled');
});
