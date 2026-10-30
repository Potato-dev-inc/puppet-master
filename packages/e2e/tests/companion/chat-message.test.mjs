// The companion's chat is the same composer as any pane's: a message reaches the agent and the answer shows.
import { noCompanions, makeCompanion, openCompanion, guarded } from './_area.mjs';
import { replyScript } from '../_helpers.mjs';

export const meta = {
  id: 'companion.chat-message',
  title: 'A message typed on the companion page reaches its agent and the reply shows',
  area: 'companion',
  tags: ['smoke'],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  t.fake.script(replyScript('pong from the companion'));
  await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);

  await t.step('Type a message in the companion chat and send it');
  await t.page.fill('Write a message for the agent', 'what is on my list today?');
  await t.click('exact=Send');

  await t.step('The agent got exactly that text');
  const got = await t.fake.waitEvent((e) => e.ev === 'prompt' && String(e.text).includes('what is on my list today?'));
  t.expect.equal(got.text, 'what is on my list today?');

  await t.step('Its reply shows in the chat');
  await t.expect.text('pong from the companion');
  await t.expect.text('Here. Ask me anything.');
});
