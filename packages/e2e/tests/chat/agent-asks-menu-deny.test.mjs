// Edge case of the same menu: the answer is Deny.
import { openChat, sendChat } from '../_helpers.mjs';
import { script } from './agent-asks-menu.test.mjs';

export const meta = {
  id: 'chat.agent-asks-menu-deny',
  title: 'Deny on the question card refuses the command',
  area: 'chat',
  tags: ['questions'],
  edge: true,
};

export async function run(t) {
  t.fake.script(script);
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'please run the command');
  await t.expect.text('Answer the agent');

  await t.step('Click Deny');
  await t.click('exact=Deny');

  await t.step('The agent reports the refusal');
  await t.expect.text('the command was refused');
  const answers = t.fake.events().filter((e) => e.ev === 'approval_answer');
  t.expect.equal(answers.length, 1, 'one answer reached the agent');
}
