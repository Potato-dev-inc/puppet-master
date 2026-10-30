// Typed shortcuts narrow the search: from:me keeps only what the person wrote.
import { scribeChat, said, openSearch, searchFor } from './_area.mjs';

export const meta = {
  id: 'search.filter-from-me',
  title: 'from:me narrows the hits to your own messages',
  area: 'search',
  tags: ['search'],
  edge: true,
};

export async function run(t) {
  await scribeChat(t, 'search-filter', [said.user('Find the orchid in the greenhouse'), said.agent('The orchid is in the greenhouse, row three.')]);
  await t.expect.text('row three');
  await openSearch(t);

  await t.step('Without a filter both lines match');
  await searchFor(t, 'orchid');
  await t.expect.count('css=.ck-palette-row--hit', 2);

  await t.step('With from:me only your message stays');
  await searchFor(t, 'from:me orchid');
  await t.expect.count('css=.ck-palette-row--hit', 1);
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-palette-row--hit .ck-hit-who').innerText`), 'You');

  await t.step('A folder that does not exist matches nothing');
  await searchFor(t, 'in:nowhere orchid');
  await t.expect.text('No matches');
}
