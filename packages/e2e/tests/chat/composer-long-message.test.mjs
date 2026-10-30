// A long message (several screen widths, no line breaks) is sent whole.
import { turnScript, think, say, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.composer-long-message',
  title: 'A long message of a thousand characters is sent whole',
  area: 'chat',
  tags: ['composer'],
  edge: true,
  timeout: 90000,
};

const TEXT = Array.from({ length: 160 }, (_, i) => `word${i}`).join(' ');

export async function run(t) {
  t.fake.script(turnScript([think(200), say('long one received')]));
  await t.spawnPane('nimbus');
  await openChat(t);
  t.expect.ok(TEXT.length > 1000, 'the test text is long');

  await t.step('Send a very long message');
  await sendChat(t, TEXT);

  await t.step('The agent received all of it');
  const prompt = await t.fake.waitEvent('prompt', { timeout: 30000 });
  t.expect.equal(prompt.text, TEXT, 'every word arrived');
  await t.expect.text('long one received');
}
