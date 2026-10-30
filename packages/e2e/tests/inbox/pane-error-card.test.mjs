// An agent that exited with an error is in the Inbox, with a card that says so.
import { openInbox, turnScript, think, say, openChat, sendChat } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.pane-error-card',
  title: 'An agent that exited with an error shows a card in the Inbox',
  area: 'inbox',
  tags: ['inbox', 'errors'],
  edge: true,
};

export async function run(t) {
  t.fake.script(turnScript([think(200), say('bye'), '{ do = "exit", code = 3 }'], { on_prompt: 'quit now' }));
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'quit now');
  await t.expect.text('The agent is not running.');

  await t.step('The Inbox lists it');
  await openInbox(t);
  await t.expect.text('One thing needs you');
  await t.expect.text(/not running|stopped|exited|error/i);
  await t.expect.noText('Nothing needs you');
}
