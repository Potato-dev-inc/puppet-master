// The line at the top of the chat names the model the agent reported; before that it says it is not known yet.
import { scribeChat, said, writeScribeChat } from './_area.mjs';

export const meta = {
  id: 'chat.turns-model-line',
  title: 'The chat header shows the model, or says it is not known yet',
  area: 'chat',
  tags: ['turns'],
};

export async function run(t) {
  const pane = await scribeChat(t, 'model');

  await t.step('Before the agent says anything the model is not known');
  await t.expect.text('Model not known yet');

  await t.step('The first answer names the model');
  writeScribeChat(pane, [said.user('hi'), said.agent('hello', { model: 'scribe-xl-2' })]);
  await t.expect.text('hello');
  await t.expect.eventually(async () => (await t.page.eval(`document.querySelector('.ck-chat-model')?.innerText`)) === 'scribe-xl-2', { message: 'the header names the model' });
  await t.expect.noText('Model not known yet');
}
