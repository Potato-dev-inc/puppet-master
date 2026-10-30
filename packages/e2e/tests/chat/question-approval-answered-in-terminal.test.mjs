// An approval answered by typing in the terminal: the card in Chat goes away by itself and the agent carries on.
import { APPROVE_FILE, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-approval-answered-in-terminal',
  title: 'An approval answered in the terminal disappears from Chat',
  area: 'chat',
  tags: ['questions', 'approval'],
  edge: true,
};

export async function run(t) {
  t.fake.script(APPROVE_FILE);
  const pane = await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'make the file');
  await t.expect.text('Answer the agent');

  await t.step('Press y in the terminal instead of clicking the card');
  await pane.write('y', { enter: false });

  await t.step('The agent carries on and the card is gone');
  await t.expect.text('file written');
  await t.expect.notVisible('exact=Allow once');
  await t.expect.noText('Answer the agent');
}
