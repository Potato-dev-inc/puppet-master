// A chat whose pane was closed is still found, as a small card with what was kept and no way to open it.
import { scribeChat, said, openSearch, searchFor } from './_area.mjs';

export const meta = {
  id: 'search.closed-chat-hit',
  title: 'A hit from a closed pane shows a card that says the chat is closed',
  area: 'search',
  tags: ['search'],
  edge: true,
};

export async function run(t) {
  const pane = await scribeChat(t, 'search-closed', [said.user('Remember the lighthouse keeper'), said.agent('The lighthouse keeper lives on the north rock.')]);
  await t.expect.text('north rock');

  await t.step('Close the pane');
  await pane.kill();
  await t.expect.text('No agents running.');

  await t.step('The words are still found');
  await openSearch(t);
  await searchFor(t, 'lighthouse');
  await t.expect.count('css=.ck-palette-row--hit', 2);

  await t.step('Choosing one shows the card');
  await t.click('css=.ck-palette-row--hit', { nth: 0 });
  await t.expect.text('This chat is closed, so this is all that was kept.');
  await t.expect.visible('exact=Back to results');

  await t.step('Back to results');
  await t.click('exact=Back to results');
  await t.expect.count('css=.ck-palette-row--hit', 2);
}
