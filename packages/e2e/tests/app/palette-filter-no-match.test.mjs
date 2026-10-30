// Edge case: typing narrows the palette's list; text that matches nothing says so; clearing it brings the list back.
import { openPalette, paletteTitles, calm } from './_area.mjs';

export const meta = {
  id: 'app.palette-filter-no-match',
  title: 'Typing narrows the palette list and text with no match says "No matches"',
  area: 'app',
  tags: ['palette'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await openPalette(t);
  const all = await paletteTitles(t);
  t.expect.ok(all.length > 10, `the palette lists many rows (${all.length})`);

  await t.step('Type "shortcuts": only the matching row stays');
  await t.type('shortcuts');
  await t.expect.eventually(async () => (await paletteTitles(t)).join('|') === 'Keyboard shortcuts', { message: 'the list is only "Keyboard shortcuts"' });

  await t.step('Type letters that match nothing');
  await t.page.fill('aria=Search commands', 'qqzzxx');
  await t.expect.text('No matches');
  t.expect.equal((await paletteTitles(t)).length, 0, 'no rows are listed');

  await t.step('Special characters and unicode do not break the search');
  await t.page.fill('aria=Search commands', '(*) [ ] ✓ 日本 \'"');
  await t.expect.text('No matches');

  await t.step('Clear the box: the whole list is back');
  await t.page.fill('aria=Search commands', '');
  await t.expect.eventually(async () => (await paletteTitles(t)).length === all.length, { message: 'every row is listed again' });
}
