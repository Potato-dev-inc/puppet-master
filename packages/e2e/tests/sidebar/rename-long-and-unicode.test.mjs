// Edge case: a very long name is cut at 40 characters; unicode and markup characters are shown as typed, never as markup.
import { calm, PANE_TITLE, paneHeading, sidebarTitles } from './_area.mjs';

export const meta = {
  id: 'sidebar.rename-long-and-unicode',
  title: 'A long name is cut at 40 characters and unicode or markup stays plain text',
  area: 'sidebar',
  tags: ['rename'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await t.click('exact=Nimbus', { within: 'css=.ck-side' });
  await t.waitFor(PANE_TITLE);

  await t.step('A name with unicode and markup characters');
  const odd = 'Fix <b>bold</b> & "quotes" ✓ 日本語';
  await t.page.doubleClick(PANE_TITLE);
  await t.page.fill('aria=Pane name', odd);
  await t.press('Enter');
  await t.expect.eventually(async () => (await paneHeading(t)) === odd, { message: 'the header shows the name exactly as typed' });
  t.expect.equal((await sidebarTitles(t)).join(), odd, 'the sidebar too');
  t.expect.equal(await t.page.eval(`document.querySelectorAll('.ck-pane-head b').length`), 0, 'the <b> was not turned into markup');

  await t.step('A name much longer than 40 characters');
  const long = 'abcdefghij'.repeat(8);
  await t.page.doubleClick(PANE_TITLE);
  await t.page.fill('aria=Pane name', long);
  await t.press('Enter');
  await t.expect.eventually(async () => (await paneHeading(t)) === long.slice(0, 40), { message: 'the name is cut at 40 characters' });
}
