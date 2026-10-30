// Quotes, brackets, a star and search words such as AND / OR typed into the box are just text: the search never errors.
import { scribeChat, said, openSearch, searchFor } from './_area.mjs';

export const meta = {
  id: 'search.odd-queries-do-not-break',
  title: 'Quotes, brackets and AND/OR in the search box do not break it',
  area: 'search',
  tags: ['search'],
  edge: true,
  timeout: 120000,
};

const ODD = ['"', '"unclosed', '(', 'a AND', 'OR OR', '*', 'NEAR(', 'col:x', "it's", '😀', '<b>', '\\'];

export async function run(t) {
  await scribeChat(t, 'search-odd', [said.user('The cat said "cornflower" (twice) to the dog'), said.agent('Fine.')]);
  await t.expect.text('Fine.');
  await openSearch(t);

  for (const q of ODD) {
    await t.step(`Search for ${q}`);
    await searchFor(t, q);
    await t.expect.eventually(
      async () => {
        const text = await t.page.text();
        return !text.includes('Searching…') && (text.includes('No matches') || (await t.page.count('css=.ck-palette-row--hit')) > 0 || text.includes('Type to search'));
      },
      { message: `the search for ${q} settled` },
    );
    await t.expect.noText('Search is not available right now.');
  }

  await t.step('A normal search still works afterwards');
  await searchFor(t, 'cornflower');
  await t.expect.count('css=.ck-palette-row--hit', 1);
}
