// A long conversation stays at its end while you are at the end, and leaves you where you are once you scrolled up.
import { scribeChat, said, writeScribeChat } from './_area.mjs';

export const meta = {
  id: 'chat.turns-long-answer-follows-end',
  title: 'The chat follows new text at the end, but not when you scrolled up',
  area: 'chat',
  tags: ['turns'],
  edge: true,
  timeout: 90000,
};

const BODY = '.ck-chat-body';
const gapToEnd = (t) => t.page.eval(`(() => { const e = document.querySelector('${BODY}'); return e.scrollHeight - e.scrollTop - e.clientHeight; })()`);
const lines = (n, prefix) => Array.from({ length: n }, (_, i) => `${prefix} line ${i + 1}`).join('\n\n');

export async function run(t) {
  const pane = await scribeChat(t, 'long', [said.user('Write a lot'), said.agent(lines(60, 'long answer'))]);
  await t.expect.text('long answer line 60');

  await t.step('The chat is at its end');
  await t.expect.eventually(async () => (await gapToEnd(t)) < 30, { message: 'the chat shows the end of the long answer' });
  t.expect.ok(await t.page.eval(`document.querySelector('${BODY}').scrollHeight > document.querySelector('${BODY}').clientHeight`), 'the answer is taller than the chat');

  await t.step('More text arrives: the chat follows it');
  writeScribeChat(pane, [said.user('And more'), said.agent(lines(10, 'second answer'))]);
  await t.expect.text('second answer line 10');
  await t.expect.eventually(async () => (await gapToEnd(t)) < 30, { message: 'the chat stayed at the end' });

  await t.step('Scroll to the top, then more text arrives');
  await t.page.eval(`document.querySelector('${BODY}').scrollTop = 0`);
  await t.expect.eventually(async () => (await gapToEnd(t)) > 200, { message: 'the chat is at the top' });
  writeScribeChat(pane, [said.user('One more'), said.agent('third answer arrived')]);
  await t.expect.eventually(async () => (await t.page.eval(`document.querySelector('${BODY}').innerText`)).includes('third answer arrived'), { message: 'the new text is in the chat' });

  await t.step('The chat did not jump to the end');
  t.expect.ok((await t.page.eval(`document.querySelector('${BODY}').scrollTop`)) < 50, 'still near the top');
}
