// Click into the terminal and type: the agent gets exactly what was typed and its answer is drawn back.
import { calm, openPane, focusTerminal } from './_area.mjs';
import { replyScript } from '../_helpers.mjs';

export const meta = {
  id: 'panes.terminal-typing',
  title: 'Typing in the terminal reaches the agent and its answer comes back',
  area: 'panes',
  tags: ['smoke', 'terminal'],
};

export async function run(t) {
  await calm(t);
  t.fake.script(replyScript('terminal answer', 200));
  await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await t.expect.text('Type a task...');

  await t.step('Click the terminal and type a task');
  await focusTerminal(t);
  await t.type('say hello');

  await t.step('The text shows in the input box before it is sent');
  await t.expect.text('> say hello');

  await t.step('Press Enter');
  await t.press('Enter');
  const prompt = await t.fake.waitEvent('prompt');
  t.expect.equal(prompt.text, 'say hello', 'the agent received what was typed');

  await t.step('The answer is drawn in the terminal');
  await t.expect.text('reply: terminal answer');
}
