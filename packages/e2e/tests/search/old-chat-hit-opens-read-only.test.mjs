// An old chat found in search opens as a read-only view; there is no message box, only the way to continue it in a pane.
import { seedOldChats, bringOldChats, openSearch, searchFor } from './_area.mjs';

export const meta = {
  id: 'search.old-chat-hit-opens-read-only',
  title: 'An old chat opens read-only and offers to continue in a pane',
  area: 'search',
  tags: ['search', 'old-chats'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await seedOldChats(t);
  await t.spawnPane('nimbus');
  await bringOldChats(t);
  await t.click('exact=Done');

  await t.step('Find the first message of an old chat');
  await openSearch(t);
  await searchFor(t, 'stripe webhook');
  await t.expect.text('Old chat · Codex CLI');
  await t.click('css=.ck-palette-row--hit', { nth: 0 });

  await t.step('The old chat opens read-only');
  await t.expect.text('Old Codex CLI chat from 14 Aug. Read-only.');
  await t.expect.text('I will add a handler in the billing module.');
  await t.expect.visible('exact=Continue in a pane');
  await t.expect.notVisible('aria=Message to the agent');
  await t.expect.noText('Messages wait until the agent is free.');
  t.expect.equal((await t.panes()).filter((p) => p.agent_type === 'codex').length, 0, 'no Codex agent was started by looking');
}
