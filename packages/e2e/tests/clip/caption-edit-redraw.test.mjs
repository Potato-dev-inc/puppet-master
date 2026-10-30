// Clip It: the request is an editable caption. Changing it offers "Draw again"; Enter draws again.
import { finishedChatTurn, makeClip } from './_area.mjs';

export const meta = {
  id: 'clip.caption-edit-redraw',
  title: 'Editing the caption offers Draw again, and drawing again keeps the new caption',
  area: 'clip',
  tags: ['clip'],
  timeout: 150000,
};

export async function run(t) {
  await finishedChatTurn(t);
  await makeClip(t);

  await t.step('Change the caption');
  await t.fill('css=.ck-clip-input', 'Greet the user');
  await t.expect.visible('exact=Draw again');

  await t.step('Press Enter in the caption: the video is drawn again');
  await t.press('Enter');
  await t.expect.text('Drawing', { timeout: 10000 }).catch(() => {});
  await t.expect.text('Look at it before you share it.', { timeout: 60000 });
  await t.expect.notVisible('exact=Draw again');
  await t.expect.value('css=.ck-clip-input', 'Greet the user');

  await t.step('Edit once more and press the Draw again button');
  await t.fill('css=.ck-clip-input', 'Greet the user politely');
  await t.click('exact=Draw again');
  await t.expect.eventually(async () => !(await t.page.has('exact=Draw again')), { message: 'Draw again goes away when the video is drawn' });
  await t.expect.text('Look at it before you share it.', { timeout: 60000 });
  await t.expect.value('css=.ck-clip-input', 'Greet the user politely');
}
