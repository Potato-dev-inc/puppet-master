// "Remove imported chats" takes the old chats out of search again (after a question), and leaves the agents' own files alone.
import fs from 'node:fs';
import path from 'node:path';
import { seedOldChats, bringOldChats, openSearch, searchFor, openBringChatsPanel, panelButtons } from './_area.mjs';

export const meta = {
  id: 'search.old-chats-remove',
  title: 'Remove imported chats takes them out of search and keeps the original files',
  area: 'search',
  tags: ['search', 'old-chats'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await seedOldChats(t);
  await t.spawnPane('nimbus');
  await bringOldChats(t);

  await t.step('Ask to remove them, then change your mind');
  await t.click('exact=Remove imported chats');
  await t.expect.text("This removes the old chats Puppet Master added to search. Your agents' own chat files are not touched.");
  await t.click('exact=Cancel');
  await t.expect.noText('This removes the old chats');

  await t.step('Remove them for real');
  await t.click('exact=Remove imported chats');
  await t.click('exact=Remove');
  await t.expect.text("Old chats removed from search. Your agents' own chat files are not touched.");
  await t.click('exact=Close');

  await t.step('Search no longer finds them');
  await openSearch(t);
  await searchFor(t, 'webhook');
  await t.expect.text('No matches');

  await t.step('Opening Bring my old chats again starts from the beginning');
  await t.press('Escape');
  await openBringChatsPanel(t);
  await t.expect.eventually(async () => (await panelButtons(t)).includes('Find my chats'), { message: 'the panel starts from its first screen' });
  await t.expect.noText('Added 5 chats');

  await t.step('The original chat files are still there');
  t.expect.ok(fs.existsSync(path.join(t.app.dirs.home, '.codex', 'sessions', '2026', '08', '14')), 'the Codex files are untouched');
}
