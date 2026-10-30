// Two agents ask at once: the Inbox counts both, the sidebar groups them under Needs you, and the count goes down as they are answered.
import { APPROVE_FILE, openInbox, openChat, sendChat } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.two-panes-count',
  title: 'Two agents asking at once show as 2 things in the Inbox',
  area: 'inbox',
  tags: ['inbox', 'two-panes'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  t.fake.script(APPROVE_FILE);
  await t.spawnPane('nimbus', { cwd: t.project('inbox-two-a') });
  await t.spawnPane('nimbus', { cwd: t.project('inbox-two-b') });

  await t.step('Ask the first agent');
  await openChat(t, 0);
  await sendChat(t, 'make the file');
  await t.expect.text('Answer the agent');

  await t.step('Ask the second agent');
  await openChat(t, 'Nimbus 2');
  await sendChat(t, 'make the file');
  await t.expect.text('Answer the agent');

  await t.step('The Inbox counts both');
  await openInbox(t);
  await t.expect.text('2 things need you');
  await t.expect.count('exact=Allow once', 2);
  await t.expect.text('Needs you', { within: 'css=.ck-side' });

  await t.step('Answer one: the count goes to one');
  await t.click('exact=Allow once', { nth: 0 });
  await t.expect.text('One thing needs you');

  await t.step('Answer the other: nothing needs you');
  await t.click('exact=Deny');
  await t.expect.text('Nothing needs you');
}
