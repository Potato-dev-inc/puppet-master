// Choosing a hit from another pane switches to that pane's Chat and shows the message.
import { installScribe, writeScribeChat, said, openSearch, searchFor, openChat } from './_area.mjs';

export const meta = {
  id: 'search.hit-opens-the-chat',
  title: 'Choosing a hit opens that pane at the message',
  area: 'search',
  tags: ['search', 'two-panes'],
  timeout: 90000,
};

export async function run(t) {
  await installScribe(t);
  const a = await t.spawnPane('scribe', { cwd: t.project('hit-a') });
  const b = await t.spawnPane('scribe', { cwd: t.project('hit-b') });
  writeScribeChat(a, [said.user('first pane talks about daffodils'), said.agent('Daffodils need cold winters.')]);
  writeScribeChat(b, [said.user('second pane talks about peonies'), said.agent('Peonies like warm rooms.')]);

  // Search holds the chats that were opened, so look at each pane's chat once.
  await t.step('Look at the second pane, then the first');
  await openChat(t, 'scribe 2');
  await t.expect.text('Peonies like warm rooms.');
  await openChat(t, 'scribe 1');
  await t.expect.text('Daffodils need cold winters.');

  await t.step('Search for a word from the second pane');
  await openSearch(t);
  await searchFor(t, 'peonies');
  await t.expect.count('css=.ck-palette-row--hit', 2);
  await t.expect.text('hit-b');

  await t.step('Choose the answer');
  await t.click('css=.ck-palette-row--hit', { nth: 0 });

  await t.step('The second pane is open on its Chat');
  await t.expect.notVisible('aria=Search all chats');
  await t.expect.text('Peonies like warm rooms.');
  await t.expect.noText('Daffodils need cold winters.');
  await t.expect.visible('aria=Message to the agent');
}
