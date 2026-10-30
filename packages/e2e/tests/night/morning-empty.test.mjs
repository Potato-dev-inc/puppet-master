// The Morning page with no night yet says how to plan one (and does not break).
import { nightSetup, resetNight } from './_area.mjs';

export const meta = {
  id: 'night.morning-empty',
  title: 'The Morning page with no night says to plan one from the plus menu',
  area: 'night',
  tags: ['night'],
  edge: true,
};

export async function run(t) {
  await t.step('Open the Morning page from the command palette');
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
  await t.type('morning');
  await t.click('exact=Morning');
  await t.waitFor('css=.ck-morning');

  await t.step('It says there is no night yet');
  await t.expect.text('No Night Shift yet. Plan tonight from the + menu.');
  await t.expect.noText('READY TO KEEP');

  await t.step('Save as page is off with nothing to save');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === 'Save as page').disabled`), true);

  await t.step('Close goes back');
  await t.click('exact=Close');
  await t.expect.notVisible('css=.ck-morning');
}

export const setup = nightSetup;
