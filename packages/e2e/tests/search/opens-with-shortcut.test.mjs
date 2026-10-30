// Ctrl+Shift+F opens "Search all chats" from anywhere; Escape closes it.
import { openSearch } from './_area.mjs';

export const meta = {
  id: 'search.opens-with-shortcut',
  title: 'Ctrl+Shift+F opens Search all chats and Escape closes it',
  area: 'search',
  tags: ['search', 'smoke'],
};

export async function run(t) {
  await t.spawnPane('nimbus');

  await t.step('Press Ctrl+Shift+F');
  await openSearch(t);
  await t.expect.text('Chats you have opened');
  await t.expect.text('Type to search every chat you have opened.');
  await t.expect.visible('exact=Agent ▾');
  await t.expect.visible('exact=Folder ▾');

  await t.step('Press Escape');
  await t.press('Escape');
  await t.expect.notVisible('aria=Search all chats');
  await t.expect.noText('Type to search every chat you have opened.');
}
