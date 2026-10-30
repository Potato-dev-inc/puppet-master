// Shift+Enter writes a new line in the box; only Enter sends. The waiting bubble keeps both lines.
import { COMPOSER, turnScript, think, say, openChat } from './_area.mjs';

export const meta = {
  id: 'chat.composer-shift-enter-newline',
  title: 'Shift+Enter adds a line, Enter sends the whole message',
  area: 'chat',
  tags: ['composer'],
  edge: true,
};

export async function run(t) {
  t.fake.script(turnScript([think(200), say('seen')]));
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Write two lines with Shift+Enter');
  await t.page.type('first line', { into: COMPOSER });
  await t.press('Shift+Enter');
  await t.page.type('second line');
  await t.expect.value(COMPOSER, 'first line\nsecond line');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 0, 'Shift+Enter sent nothing');

  await t.step('Enter sends it');
  await t.press('Enter');
  await t.expect.value(COMPOSER, '');
  const bubble = await t.expect.eventually(
    async () => (await t.page.eval(`document.querySelector('[data-testid=chat-pending] .ck-chat-bubble')?.innerText || ''`)) || null,
    { message: 'the message shows as a bubble' },
  );
  t.expect.equal(bubble, 'first line\nsecond line', 'the bubble keeps both lines');
  t.expect.includes((await t.fake.waitEvent('prompt')).text, 'first line');
}
