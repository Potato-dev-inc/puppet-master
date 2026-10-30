// The search box narrows the cards by name and words; a word that matches nothing says so, and clearing brings everything back.
import { openStore, storeCards } from '../night/_area.mjs';

export const meta = {
  id: 'store.search-narrows-cards',
  title: 'Searching the Agent Store narrows the cards and an empty search says so',
  area: 'store',
  tags: ['store'],
  timeout: 90000,
};

export async function run(t) {
  await openStore(t);
  const total = (await storeCards(t)).length;

  await t.step('Type part of a name');
  await t.fill('aria=Search agents', 'gemini');
  await t.expect.eventually(async () => (await storeCards(t)).length === 1, { message: 'one card is left' });
  t.expect.ok((await storeCards(t))[0].startsWith('Gemini CLI:'), 'it is the Gemini card');

  await t.step('Search by the maker, in capitals and with spaces around');
  await t.fill('aria=Search agents', '  ANTHROPIC ');
  await t.expect.eventually(async () => (await storeCards(t)).some((c) => c.startsWith('Claude Code:')), { message: 'Claude Code is found by its maker' });

  await t.step('A word nobody has: the page says nothing matches');
  await t.fill('aria=Search agents', 'zzzqqq');
  await t.expect.text('No agents match. Try another word, or press All.');
  t.expect.equal((await storeCards(t)).length, 0);

  await t.step('Clear the search: every card is back');
  await t.fill('aria=Search agents', '');
  await t.expect.eventually(async () => (await storeCards(t)).length === total, { message: 'all the cards are back' });
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
