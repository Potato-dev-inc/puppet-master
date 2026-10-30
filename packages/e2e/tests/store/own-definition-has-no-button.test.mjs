// An agent the person defined in their own file (here the fake Nimbus) is left alone: its card explains and has no button.
import { openStore } from '../night/_area.mjs';

export const meta = {
  id: 'store.own-definition-has-no-button',
  title: 'A card for an agent you defined yourself has no button',
  area: 'store',
  tags: ['store'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await openStore(t);

  await t.step('The Nimbus card says the Store leaves it alone');
  await t.expect.visible({ exact: 'Nimbus', within: 'css=.ck-store-grid' });
  await t.expect.text('You have your own nimbus definition, so the Store leaves it alone.');

  await t.step('The card has no button at all');
  const buttons = await t.page.eval(`(() => { const c = [...document.querySelectorAll('.ck-store-card')].find((x) => x.querySelector('.ck-row-title').innerText === 'Nimbus'); return c ? c.querySelectorAll('button').length : -1; })()`);
  t.expect.equal(buttons, 0);
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
