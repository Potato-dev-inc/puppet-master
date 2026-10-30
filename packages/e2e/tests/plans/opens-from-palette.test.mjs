// The command palette has a row for the plans page; Close leaves it.
import { sclick } from '../night/_area.mjs';

export const meta = {
  id: 'plans.opens-from-palette',
  title: 'The command palette opens Your plans and Close leaves it',
  area: 'plans',
  tags: ['plans'],
};

export async function run(t) {
  await t.step('Open the palette and look for the plans page');
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
  await t.type('plans');
  await t.expect.visible({ text: 'Your plans', within: 'css=[aria-label="Command palette"]' });

  await t.step('Pick the row');
  await t.click({ text: 'Your plans', within: 'css=[aria-label="Command palette"]' });
  await t.waitFor('exact=Your plans this month');
  await t.expect.visible('aria=Earlier month');

  await t.step('Close goes back to the Inbox');
  await sclick(t, 'exact=Close');
  await t.expect.notVisible('exact=Your plans this month');
  await t.expect.text('No agents running.');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
