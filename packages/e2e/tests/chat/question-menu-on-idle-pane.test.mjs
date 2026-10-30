// A numbered menu with a cursor that a pane shows while idle becomes a card in Chat. A click on a row answers it with arrow keys.
import { turnScript, think, hold, keysSeen, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-menu-on-idle-pane',
  title: 'A menu on an idle pane becomes a card and a click on a row answers it',
  area: 'chat',
  tags: ['questions', 'smoke'],
};

export async function run(t) {
  t.fake.script(turnScript([think(300), hold('Which flavour?\n> 1. Alpha\n  2. Beta\n  3. Gamma')]));
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('The agent draws a menu and stops');
  await sendChat(t, 'go');

  await t.step('The card asks with the menu rows as buttons');
  await t.expect.text('Agent is asking');
  await t.expect.visible('exact=Alpha');
  await t.expect.visible('exact=Beta');
  await t.expect.visible('exact=Gamma');
  await t.expect.visible('exact=Open Terminal');

  await t.step('Click the third row');
  await t.click('exact=Gamma');

  await t.step('The cursor was moved down twice, then Enter');
  await t.expect.eventually(() => keysSeen(t).join(',') === 'Enter,Down,Down,Enter', {
    message: () => `expected the keys Enter (the message), Down, Down, Enter. The agent saw: ${keysSeen(t).join(',')}`,
  });
}
