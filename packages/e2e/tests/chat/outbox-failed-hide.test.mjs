// A failed message can be hidden; Copy text answers with a note.
import { turnScript, think, say, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.outbox-failed-hide',
  title: 'Hide takes a failed message off the list',
  area: 'chat',
  tags: ['outbox', 'errors'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  t.fake.script(turnScript([think(2500), say('bye'), '{ do = "exit", code = 1 }'], { on_prompt: 'quit soon' }));
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'quit soon');
  await t.fake.waitEvent('prompt');
  await sendChat(t, 'lost words');
  await t.expect.text('The agent is not running in this pane.');

  await t.step('Copy the text of the failed message');
  await t.click('exact=Copy text');
  await t.expect.eventually(async () => /Copied\.|Could not copy\./.test(await t.page.text()), { message: 'Copy text answers with a note' });

  await t.step('Hide it');
  await t.click('exact=Hide');
  await t.expect.noText('The agent is not running in this pane.');
  await t.expect.noText('lost words');
}
