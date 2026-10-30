// A menu with no numbers, only a cursor row: picking a row moves the cursor with the arrow keys first.
import { turnScript, think, hold, keysSeen, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-pointer-menu-arrow-keys',
  title: 'A menu with only a cursor is answered with arrow keys',
  area: 'chat',
  tags: ['questions'],
};

export async function run(t) {
  t.fake.script(turnScript([think(300), hold('Pick a theme\n\n> Dark\n  Light\n  Auto')]));
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'go');

  await t.step('The three themes show as buttons');
  await t.expect.text('Agent is asking');
  await t.expect.visible('exact=Dark');
  await t.expect.visible('exact=Light');
  await t.expect.visible('exact=Auto');

  await t.step('Pick the second theme');
  await t.click('exact=Light');
  await t.expect.eventually(() => keysSeen(t).join(',') === 'Enter,Down,Enter', {
    message: () => `expected Down then Enter after the message. The agent saw: ${keysSeen(t).join(',')}`,
  });
}
