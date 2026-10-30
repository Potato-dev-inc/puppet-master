// A PC where no agent kept any chat: every agent says 0 chats and there is nothing to add.
import { openBringChats, resetOldChats } from './_area.mjs';

export const meta = {
  id: 'search.old-chats-none-found',
  title: 'With no old chats on the PC every agent says 0 chats and nothing can be added',
  area: 'search',
  tags: ['search', 'old-chats'],
  edge: true,
};

export async function run(t) {
  await resetOldChats(t);
  await t.spawnPane('nimbus');

  await t.step('Find my chats on a PC with no history');
  await openBringChats(t);
  await t.expect.text('Claude Code · 0 chats');
  await t.expect.text('Codex CLI · 0 chats');
  await t.expect.text('Add 0 chats to search');
  t.expect.ok(await t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === 'Add 0 chats to search').disabled`), 'there is nothing to add');

  await t.step('Close leaves without changing anything');
  await t.click('exact=Close');
  await t.expect.noText('Claude Code · 0 chats');
}
