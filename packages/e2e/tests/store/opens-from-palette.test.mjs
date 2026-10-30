// The command palette has "Open Agent Store"; "Set up agents" and Settings > Agents link to it too.
import { sclick } from '../night/_area.mjs';

export const meta = {
  id: 'store.opens-from-palette',
  title: 'The command palette opens the Agent Store and Back returns',
  area: 'store',
  tags: ['store'],
  timeout: 90000,
};

export async function run(t) {
  await t.step('Open the command palette and look for the Store');
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
  await t.type('store');
  await t.expect.text('Open Agent Store');

  await t.step('Pick it');
  await t.click('exact=Open Agent Store');
  await t.waitFor('exact=Agent Store');
  await t.waitFor('css=.ck-store-grid', { timeout: 45000 });

  await t.step('Go back');
  await sclick(t, 'exact=Back');
  await t.expect.notVisible('exact=Agent Store');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
