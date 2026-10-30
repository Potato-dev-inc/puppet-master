// Looking again after the old chats were added finds nothing new: each agent says how many are already in search.
import { seedOldChats, bringOldChats } from './_area.mjs';

export const meta = {
  id: 'search.old-chats-look-again',
  title: 'Look again says the old chats are already in search and adds nothing',
  area: 'search',
  tags: ['search', 'old-chats'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await seedOldChats(t);
  await t.spawnPane('nimbus');
  await bringOldChats(t);

  await t.step('Look again');
  await t.click('exact=Look again');
  await t.expect.text('2 already in search');
  await t.expect.text('3 already in search');
  await t.expect.text('Add 0 chats to search');
  t.expect.ok(await t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === 'Add 0 chats to search').disabled`), 'the Add button is off when there is nothing new');
}
