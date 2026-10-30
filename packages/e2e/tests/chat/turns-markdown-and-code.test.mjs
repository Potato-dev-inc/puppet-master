// The agent's answer is shown as formatted text: bold, a list, a code block. The marks are not shown as symbols.
import { scribeChat, said, writeScribeChat } from './_area.mjs';

export const meta = {
  id: 'chat.turns-markdown-and-code',
  title: 'An answer written in Markdown shows as formatted text with a code block',
  area: 'chat',
  tags: ['turns', 'smoke'],
};

export async function run(t) {
  const pane = await scribeChat(t, 'markdown');

  await t.step('The chat of a fresh pane is empty and says it waits');
  await t.expect.text('Waiting for a message on the terminal');

  await t.step('The agent answers with Markdown');
  writeScribeChat(pane, [
    said.user('Explain the plan'),
    said.agent('## The plan\n\nI will **read** the file, then `edit` it:\n\n- first step\n- second step\n\n```js\nconst answer = 42;\nconsole.log(answer);\n```\n\nSee https://example.com/docs for more.'),
  ]);
  await t.expect.text('The plan');
  await t.expect.text('first step');
  await t.expect.text('const answer = 42;');

  await t.step('The marks are gone: no stars, no hash signs, no backticks, no list dashes');
  await t.expect.noText('**read**');
  await t.expect.noText('## The plan');
  await t.expect.noText('```');
  await t.expect.noText('- first step');
  t.expect.ok(await t.page.eval(`!!document.querySelector('.ck-chat strong')`), 'the bold word is bold');
  t.expect.ok(await t.page.eval(`document.querySelectorAll('.ck-chat li').length === 2`), 'the list has two items');
  t.expect.ok(await t.page.eval(`!!document.querySelector('.ck-chat pre')`), 'the code sits in a code block');
}
