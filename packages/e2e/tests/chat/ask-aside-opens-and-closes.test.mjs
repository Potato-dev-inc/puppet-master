// "Ask aside" opens a small box over the chat for a question about the session. It never writes into the pane.
import { openChat } from './_area.mjs';
// This test only opens and closes the box: asking would start a real helper agent, which tests never do.

export const meta = {
  id: 'chat.ask-aside-opens-and-closes',
  title: 'Ask aside opens a box that says the agent does not see the question',
  area: 'chat',
  tags: ['ask-aside'],
};

export async function run(t) {
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Click Ask aside');
  await t.click('exact=Ask aside');
  await t.expect.text('Ask about this session');
  await t.expect.text('A helper reads the recent chat and answers here. Your agent does not see this.');
  await t.expect.visible('aria=Your question');

  await t.step('Ask is off until a question is typed');
  t.expect.ok(await t.page.eval(`[...document.querySelectorAll('[data-testid=side-question] button')].find((b) => b.innerText.trim() === 'Ask').disabled`), 'Ask is off for an empty question');

  await t.step('Escape closes the box');
  await t.press('Escape');
  await t.expect.notVisible('css=[data-testid=side-question]');

  await t.step('Ctrl+; opens it again from the keyboard');
  await t.page.type('x', { into: 'aria=Message to the agent' });
  await t.press('Ctrl+;');
  await t.expect.visible('css=[data-testid=side-question]');
  await t.click('aria=Close side question');
  await t.expect.notVisible('css=[data-testid=side-question]');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 0, 'the agent was never told anything');
}
