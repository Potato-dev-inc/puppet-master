// The same y/n approval answered with Deny: nothing is written.
import fs from 'node:fs';
import path from 'node:path';
import { APPROVE_FILE, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-approval-yn-deny',
  title: 'Deny on a y/n approval keeps the file from being created',
  area: 'chat',
  tags: ['questions', 'approval'],
  edge: true,
};

export async function run(t) {
  t.fake.script(APPROVE_FILE);
  const pane = await t.spawnPane('nimbus', { cwd: t.project('refuse-file-project') });
  await openChat(t);
  await sendChat(t, 'make the file');
  await t.expect.text('Answer the agent');

  await t.step('Deny');
  await t.click('exact=Deny');

  await t.step('The agent reports it and no file exists');
  await t.expect.text('nothing written');
  await t.expect.notVisible('exact=Deny');
  t.expect.ok(!fs.existsSync(path.join(pane.cwd, 'hello.txt')), 'hello.txt was not created');
}
