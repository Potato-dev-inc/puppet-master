// The bubble of a message just sent is replaced by the real message once the agent's conversation shows it.
import { scribeChat, said, writeScribeChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.turns-pending-bubble-replaced',
  title: 'The sent bubble gives way to the real message in the conversation',
  area: 'chat',
  tags: ['turns', 'outbox'],
  edge: true,
};

export async function run(t) {
  const pane = await scribeChat(t, 'bubble');

  await t.step('Send a message: it shows as a bubble marked Sent');
  await sendChat(t, 'please add tests');
  await t.expect.eventually(() => t.page.eval(`document.querySelector('[data-testid=chat-pending]')?.dataset.state === 'sent'`), { message: 'the bubble says it was sent' });
  await t.expect.text('Sent');
  await t.expect.count('css=[data-testid=chat-pending]', 1);

  await t.step('The agent records the message: the bubble is replaced');
  writeScribeChat(pane, [said.user('please add tests'), said.agent('Adding them now.')]);
  await t.expect.text('Adding them now.');
  await t.expect.count('css=[data-testid=chat-pending]', 0);
  await t.expect.count('text=please add tests', 1, { within: 'css=.ck-chat-body' });
}
