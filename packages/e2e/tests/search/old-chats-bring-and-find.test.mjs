// Bring All My Chats: the chats other agents kept on this PC are read into search when the person asks, and found with an "Old chat" tag.
// The agents' history folders are the synthetic fixtures of the repo, copied into the test app's own home folder.
import { seedOldChats, openBringChats, openSearch, searchFor, clickWhenStill, panelStart, clickPanelButton } from './_area.mjs';

export const meta = {
  id: 'search.old-chats-bring-and-find',
  title: 'Old chats are brought into search and found with an Old chat tag',
  area: 'search',
  tags: ['search', 'old-chats', 'smoke'],
  timeout: 90000,
};

export async function run(t) {
  await seedOldChats(t);
  await t.spawnPane('nimbus');

  await t.step('Open Bring my old chats from the command palette');
  await t.press('Ctrl+K');
  await t.page.type('old chats');
  await t.expect.text('Read the chats you had in other agents, into search. Nothing leaves this PC.');
  await t.press('Enter');
  const start = await panelStart(t);
  // The first screen explains what happens; it shows only while no import ran since the app started.
  if (start === 'Find my chats') {
    await t.expect.text('Read my old chats on this PC. Nothing leaves this computer.');
    await t.expect.text('Secrets are hidden before anything is saved.');
  }

  await t.step('Find my chats: it lists what each agent kept');
  await clickPanelButton(t, start);
  await t.expect.text('Claude Code · 2 chats');
  await t.expect.text('Codex CLI · 3 chats');
  await t.expect.text("Can't read its old chats yet");

  await t.step('Add them to search');
  await clickWhenStill(t, 're=/Add 5 chats to search/');
  await t.expect.text('Added 5 chats. Press Ctrl+Shift+F to find anything in them.', { timeout: 30000 });
  await t.click('exact=Done');

  await t.step('Search for a word from an old chat');
  await openSearch(t);
  await searchFor(t, 'webhook');
  await t.expect.count('css=.ck-palette-row--hit', 3);
  await t.expect.text('Old chat · Codex CLI · shop');
  await t.expect.text('Add a stripe webhook for paid invoices.');
}
