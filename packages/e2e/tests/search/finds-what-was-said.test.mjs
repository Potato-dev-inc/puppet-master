// Search finds words from a chat, says who said them and where, and marks the match.
import { scribeChat, said, openSearch, searchFor } from './_area.mjs';

export const meta = {
  id: 'search.finds-what-was-said',
  title: 'Searching a word finds both your message and the answer',
  area: 'search',
  tags: ['search', 'smoke'],
};

export async function run(t) {
  await scribeChat(t, 'search-zebra', [
    said.user('Where is the zebra crossing?'),
    said.agent('The zebra crossing is on Main Street, next to the bakery.'),
    said.user('Thanks, and the library?'),
    said.agent('The library is opposite the park.'),
  ]);
  await t.expect.text('opposite the park');

  await t.step('Open the search and type a word');
  await openSearch(t);
  await searchFor(t, 'zebra');

  await t.step('Two hits: the question and the answer, each with who said it and where');
  await t.expect.count('css=.ck-palette-row--hit', 2);
  await t.expect.text('Where is the zebra crossing?');
  await t.expect.text('The zebra crossing is on Main Street');
  await t.expect.text('scribe · scribe-search-zebra');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('.ck-palette-row--hit .ck-hit-who')].map((e) => e.innerText).sort().join(',')`), 'Agent,You');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('.ck-palette-row--hit mark')].map((m) => m.innerText.toLowerCase()).join(',')`), 'zebra,zebra', 'the matching word is marked in each hit');

  await t.step('Another word finds the other two lines');
  await searchFor(t, 'library');
  const hitsText = () => t.page.eval(`[...document.querySelectorAll('.ck-palette-row--hit')].map((e) => e.innerText).join(' | ')`);
  await t.expect.eventually(
    async () => {
      const text = await hitsText();
      return text.includes('The library is opposite the park.') && text.includes('Thanks, and the library?') && !text.includes('zebra crossing');
    },
    { message: 'the two library lines are the hits and no zebra crossing line' },
  );
  await t.expect.count('css=.ck-palette-row--hit', 2);
}
