// With one month of history the Earlier and Later month buttons are off and the month list has one entry.
export const meta = {
  id: 'plans.one-month-no-navigation',
  title: 'With a single month the month arrows are off',
  area: 'plans',
  tags: ['plans'],
  edge: true,
};

export async function run(t) {
  await t.click('exact=Your plans');
  await t.waitFor('exact=Your plans this month');
  await t.waitFor('aria=Earlier month');

  await t.step('Both arrows are off');
  await t.expect.eventually(() => t.page.eval(`document.querySelector('button[aria-label="Earlier month"]').disabled`), { message: 'Earlier month is disabled' });
  await t.expect.eventually(() => t.page.eval(`document.querySelector('button[aria-label="Later month"]').disabled`), { message: 'Later month is disabled' });

  await t.step('The month list has this month only');
  const options = await t.page.eval(`[...document.querySelector('select[aria-label="Month"]').options].map((o) => o.text)`);
  t.expect.equal(options.length, 1);

  await t.step('Pressing a disabled arrow changes nothing');
  await t.click('aria=Earlier month');
  await t.expect.text('Your plans this month');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
