// Text from the agent is never run: tags show as typed, an emoji and non-Latin letters stay intact.
import { scribeChat, said } from './_area.mjs';

export const meta = {
  id: 'chat.turns-special-text',
  title: 'HTML-looking text, an emoji and non-Latin letters from the agent show as typed',
  area: 'chat',
  tags: ['turns'],
  edge: true,
};

export async function run(t) {
  await scribeChat(t, 'special', [
    said.user('Say <b>hi</b> in other languages 🌍'),
    said.agent('Here: `<script>window.__pwned = 1</script>` and <img src=x onerror="window.__pwned = 2"> then こんにちは, Здравствуйте, مرحبا 🎉'),
  ]);

  await t.step('Your message and the answer show every character');
  await t.expect.text('Say <b>hi</b> in other languages 🌍');
  await t.expect.text('こんにちは');
  await t.expect.text('Здравствуйте');
  await t.expect.text('🎉');
  await t.expect.text('<script>window.__pwned = 1</script>');

  await t.step('Nothing in it ran or became an element');
  t.expect.equal(await t.page.eval('window.__pwned === undefined'), true, 'no script ran');
  t.expect.equal(await t.page.eval(`document.querySelectorAll('.ck-chat-body img, .ck-chat-body script').length`), 0, 'no image or script element was made');
}
