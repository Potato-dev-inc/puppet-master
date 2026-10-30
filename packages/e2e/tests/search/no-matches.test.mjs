// A word nobody said gives a plain "No matches", not an error.
import { scribeChat, said, openSearch, searchFor } from './_area.mjs';

export const meta = {
  id: 'search.no-matches',
  title: 'A word that is in no chat says No matches',
  area: 'search',
  tags: ['search'],
  edge: true,
};

export async function run(t) {
  await scribeChat(t, 'search-none', [said.user('Hello there'), said.agent('General Kenobi.')]);
  await t.expect.text('General Kenobi.');
  await openSearch(t);
  await searchFor(t, 'xylophone');
  await t.expect.text('No matches');
  await t.expect.noText('Search is not available right now.');
  await t.expect.count('css=.ck-palette-row--hit', 0);

  await t.step('A word that is there finds it again');
  await searchFor(t, 'kenobi');
  await t.expect.count('css=.ck-palette-row--hit', 1);
  await t.expect.noText('No matches');
}
