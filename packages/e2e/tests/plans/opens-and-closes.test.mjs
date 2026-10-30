// "Your plans" in the sidebar opens a page that says what each plan did this month; with no turns yet it says so.
import { sclick } from '../night/_area.mjs';

export const meta = {
  id: 'plans.opens-and-closes',
  title: 'Your plans opens a month page with its notes and Close leaves it',
  area: 'plans',
  tags: ['plans', 'smoke'],
};

export async function run(t) {
  await t.step('Press Your plans in the sidebar');
  await t.click('exact=Your plans');
  await t.waitFor('exact=Your plans this month');
  await t.expect.text('What each plan did, side by side.');

  await t.step('The month is shown, with nothing counted yet');
  const month = await t.page.eval(`new Date().toLocaleString('en-US', { month: 'long', year: 'numeric' })`);
  await t.expect.text(month);
  // A new profile has no turns; a run that already had agents work shows a table. Either is the right page.
  await t.expect.eventually(async () => {
    const text = await t.page.text();
    return text.includes('No turns this month yet.') || (text.includes('Turns') && text.includes('Merged'));
  }, { message: 'the page shows its empty words or its table' });

  await t.step('The notes say what is and is not counted');
  await t.expect.text('Counting limits since');
  await t.expect.text('Earlier limits were not recorded.');
  await t.expect.text('Tokens are not counted. Only usage an agent reports itself would show here.');

  await t.step('Close goes back to the Inbox');
  await sclick(t, 'exact=Close');
  await t.expect.notVisible('exact=Your plans this month');
  await t.expect.text('No agents running.');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
