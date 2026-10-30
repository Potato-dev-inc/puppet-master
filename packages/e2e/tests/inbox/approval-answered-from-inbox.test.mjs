// The agent's approval shows in the Inbox too: answer it there, without opening the pane.
import fs from 'node:fs';
import path from 'node:path';
import { APPROVE_FILE, openInbox, openChat, sendChat } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.approval-answered-from-inbox',
  title: 'An approval is answered from the Inbox and the agent carries on',
  area: 'inbox',
  tags: ['inbox', 'approval', 'smoke'],
};

export async function run(t) {
  t.fake.script(APPROVE_FILE);
  const pane = await t.spawnPane('nimbus', { cwd: t.project('inbox-approval') });
  await openChat(t);
  await sendChat(t, 'make the file');
  await t.expect.text('Answer the agent');

  await t.step('Open the Inbox: one thing needs you');
  await openInbox(t);
  await t.expect.text('One thing needs you');
  await t.expect.text('Answer the agent');
  await t.expect.visible('exact=Allow once');

  await t.step('Allow once from the Inbox');
  await t.click('exact=Allow once');

  await t.step('The agent wrote the file and nothing is waiting');
  await t.fake.waitEvent('approval_answer');
  await t.expect.text('Nothing needs you');
  await t.expect.eventually(() => fs.existsSync(path.join(pane.cwd, 'hello.txt')), { message: 'hello.txt was created' });
}
