// The command palette has an action for the same search; Backspace on an empty box goes back to the commands.
import { scribeChat, said, searchFor } from './_area.mjs';

export const meta = {
  id: 'search.from-command-palette',
  title: 'Search all chats can be reached from the command palette',
  area: 'search',
  tags: ['search', 'keyboard'],
  edge: true,
};

export async function run(t) {
  await scribeChat(t, 'search-palette', [said.user('Plan the picnic'), said.agent('Bring sandwiches to the picnic.')]);
  await t.expect.text('Bring sandwiches');

  await t.step('Open the command palette and pick Search all chats');
  await t.press('Ctrl+K');
  await t.expect.visible('aria=Search commands');
  await t.page.type('search all');
  await t.expect.text('Search all chats');
  await t.press('Enter');

  await t.step('The same search box opens and works');
  await t.waitFor('aria=Search all chats');
  await t.expect.text('Type to search every chat you have opened.');
  await searchFor(t, 'sandwiches');
  await t.expect.count('css=.ck-palette-row--hit', 1);

  await t.step('Empty the box and press Backspace: back to the commands');
  await searchFor(t, '');
  await t.press('Backspace');
  await t.waitFor('aria=Search commands');
}
