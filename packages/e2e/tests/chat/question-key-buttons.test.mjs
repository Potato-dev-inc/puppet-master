// The card has plain key buttons for what a menu may need: Up, Down, Enter, Esc and Tab.
import { turnScript, think, hold, keysSeen, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-key-buttons',
  title: 'Up, Down, Enter, Esc and Tab buttons press those keys in the agent',
  area: 'chat',
  tags: ['questions'],
};

export async function run(t) {
  t.fake.script(turnScript([think(300), hold('Which flavour?\n> 1. Alpha\n  2. Beta')]));
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'go');
  await t.expect.text('Agent is asking');

  await t.step('Press the five key buttons one after the other');
  for (const key of ['Up', 'Down', 'Enter', 'Esc', 'Tab']) await t.click(`exact=${key}`);

  await t.step('The agent saw the same keys');
  await t.expect.eventually(() => keysSeen(t).join(',') === 'Enter,Up,Down,Enter,Esc,Tab', {
    message: () => `the agent saw: ${keysSeen(t).join(',')}`,
  });
}
