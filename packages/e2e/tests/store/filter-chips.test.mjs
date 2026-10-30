// The filter chips narrow the grid; the pressed chip is marked; All brings everything back.
import { openStore, storeCards } from '../night/_area.mjs';

export const meta = {
  id: 'store.filter-chips',
  title: 'The Tested and Open source chips narrow the Agent Store',
  area: 'store',
  tags: ['store'],
  timeout: 90000,
};

const pressed = (t) => t.page.eval(`[...document.querySelectorAll('.ck-store-chips button')].filter((b) => b.getAttribute('aria-pressed') === 'true').map((b) => b.innerText)`);
const every = (t, re) => t.page.eval(`[...document.querySelectorAll('.ck-store-card')].every((c) => ${re}.test(c.innerText))`);

export async function run(t) {
  await openStore(t);
  const total = (await storeCards(t)).length;
  t.expect.equal(await pressed(t), ['All']);

  await t.step('Press Tested: only agents the team tested stay');
  await t.click({ exact: 'Tested', within: 'css=.ck-store-chips' });
  await t.expect.eventually(async () => (await storeCards(t)).length < total, { message: 'fewer cards' });
  t.expect.equal(await pressed(t), ['Tested']);
  t.expect.equal(await every(t, '/Tested/'), true, 'every card is tested');
  t.expect.ok((await storeCards(t)).some((c) => c.startsWith('Claude Code:')), 'Claude Code is tested');

  await t.step('Press Open source: the paid-terms agents leave');
  await t.click({ exact: 'Open source', within: 'css=.ck-store-chips' });
  await t.expect.eventually(async () => (await pressed(t)).join() === 'Open source', { message: 'only Open source is pressed' });
  t.expect.equal(await every(t, '/Open source/'), true, 'every card is open source');
  t.expect.ok(!(await storeCards(t)).some((c) => c.startsWith('Claude Code:')), 'Claude Code is not open source');

  await t.step('Filter and search together');
  await t.fill('aria=Search agents', 'qwen');
  await t.expect.eventually(async () => (await storeCards(t)).length === 1, { message: 'one open source agent matches qwen' });

  await t.step('A filter with a search nobody matches is empty; All and a clean search restore it');
  await t.fill('aria=Search agents', 'claude');
  await t.expect.text('No agents match.');
  await t.fill('aria=Search agents', '');
  await t.click({ exact: 'All', within: 'css=.ck-store-chips' });
  await t.expect.eventually(async () => (await storeCards(t)).length === total, { message: 'all the cards are back' });
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
