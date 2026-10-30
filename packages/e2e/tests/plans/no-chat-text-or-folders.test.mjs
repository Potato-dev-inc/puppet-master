// The page never shows chat text or folder names, even after an agent has worked in a named folder.
import { gitProject, openChatOf, sclick } from '../night/_area.mjs';
import { replyScript, sendChat } from '../_helpers.mjs';

export const meta = {
  id: 'plans.no-chat-text-or-folders',
  title: 'The plans page shows no chat words and no folder names',
  area: 'plans',
  tags: ['plans'],
  edge: true,
};

export async function run(t) {
  t.fake.script(replyScript('reply with a very private sentence'));
  await t.spawnPane('nimbus', { cwd: gitProject(t, 'secret-folder-name') });
  await openChatOf(t, 'Nimbus');
  await sendChat(t, 'my own distinctive question about walruses');
  await t.expect.text('reply with a very private sentence');

  await t.step('Open Your plans');
  await sclick(t, 'exact=Your plans');
  await t.waitFor('exact=Your plans this month');
  await t.waitFor('aria=Earlier month');

  await t.step('The page has neither the question, the answer nor the folder');
  const text = await t.page.eval(`document.querySelector('.ck-plans').innerText`);
  for (const word of ['walruses', 'very private sentence', 'secret-folder-name']) {
    t.expect.ok(!text.includes(word), `the plans page does not show "${word}"`);
  }
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
