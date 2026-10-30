// The Transcript tab is for work another agent handed to this one. What you typed yourself is not there.
import { replyScript } from '../_helpers.mjs';
import { openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.tabs-transcript-empty',
  title: 'The Transcript tab says what it keeps when there is nothing in it',
  area: 'chat',
  tags: ['tabs'],
  edge: true,
};

export async function run(t) {
  t.fake.script(replyScript('typed here'));
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'a message I typed');
  await t.expect.text('typed here');

  await t.step('Open the Transcript tab');
  await t.click('exact=Transcript');
  await t.expect.text('No transcript');
  await t.expect.text('A transcript is kept for tasks that another agent handed to this one.');
  await t.expect.noText('reply: typed here');

  await t.step('The tab bar still works: back to Chat');
  await t.click('exact=Chat');
  await t.expect.visible('aria=Message to the agent');
}
