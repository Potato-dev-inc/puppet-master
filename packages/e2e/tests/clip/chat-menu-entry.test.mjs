// Clip It: the chat menu has the same action as the button on the end line.
import { finishedChatTurn } from './_area.mjs';

export const meta = {
  id: 'clip.chat-menu-entry',
  title: 'The chat menu offers "Make clip of last turn" and it opens the same sheet',
  area: 'clip',
  tags: ['clip'],
  timeout: 90000,
};

export async function run(t) {
  await finishedChatTurn(t);

  await t.step('Open the chat menu');
  await t.click('aria=Chat menu');
  await t.expect.visible('Make clip of last turn');

  await t.step('Choose Make clip of last turn');
  await t.click('Make clip of last turn');
  await t.expect.text('Make a clip');
  await t.expect.value('css=.ck-clip-input', 'Add a greeting to main.py');
  await t.expect.text('Look at it before you share it.', { timeout: 60000 });

  await t.step('Close leaves the sheet and saves nothing');
  await t.click('exact=Close');
  await t.expect.notVisible('Make a clip');
}
