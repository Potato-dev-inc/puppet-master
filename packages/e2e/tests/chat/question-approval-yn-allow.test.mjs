// A one-line [y/n] approval to create a file: Allow once lets the agent write it.
import fs from 'node:fs';
import path from 'node:path';
import { APPROVE_FILE, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-approval-yn-allow',
  title: 'Allow once on a y/n approval lets the agent create the file',
  area: 'chat',
  tags: ['questions', 'approval'],
};

export async function run(t) {
  t.fake.script(APPROVE_FILE);
  const pane = await t.spawnPane('nimbus', { cwd: t.project('approval-yn-allow') });
  await openChat(t);
  await sendChat(t, 'make the file');

  await t.step('The card asks and the file does not exist yet');
  await t.expect.text('Answer the agent');
  await t.expect.visible('exact=Allow once');
  await t.expect.visible('exact=Deny');
  t.expect.ok(!fs.existsSync(path.join(pane.cwd, 'hello.txt')), 'nothing was written before the answer');

  await t.step('Allow once');
  await t.click('exact=Allow once');

  await t.step('The agent writes the file');
  await t.expect.text('file written');
  t.expect.ok(fs.existsSync(path.join(pane.cwd, 'hello.txt')), 'hello.txt exists');
  await t.fake.waitEvent('approval_answer');
}
