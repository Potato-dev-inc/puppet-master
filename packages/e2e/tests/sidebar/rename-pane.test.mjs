// Double-click the title (or right-click its row > Rename) to name a pane. The name shows in the sidebar, the header and the palette.
import { calm, PANE_TITLE, paneHeading, openPalette, paletteTitles, sidebarTitles } from './_area.mjs';

export const meta = {
  id: 'sidebar.rename-pane',
  title: 'Renaming a pane shows the new name in the header, sidebar and palette',
  area: 'sidebar',
  tags: ['smoke', 'rename'],
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await t.click('exact=Nimbus', { within: 'css=.ck-side' });
  await t.waitFor(PANE_TITLE);

  await t.step('Pane menu > Rename');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('exact=Rename');
  await t.waitFor('aria=Pane name');

  await t.step('Type a name and press Enter');
  await t.page.fill('aria=Pane name', 'Login page work');
  await t.press('Enter');

  await t.step('The header and the sidebar use the name');
  await t.expect.eventually(async () => (await paneHeading(t)) === 'Login page work', { message: 'the header shows the new name' });
  await t.expect.eventually(async () => (await sidebarTitles(t)).join() === 'Login page work', { message: 'the sidebar shows the new name' });
  await t.expect.text('Nimbus · Idle');

  await t.step('The palette finds it by its name');
  await openPalette(t);
  await t.type('login');
  t.expect.includes(await paletteTitles(t), 'Login page work');
  await t.press('Escape');
}
