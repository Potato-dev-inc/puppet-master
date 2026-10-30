// The card follows the screen: answer the question in the terminal and the card goes away by itself.
import { COMPOSER, openChat, sendChat } from './_area.mjs';
import { turnScript, think, hold, say } from './_area.mjs';

export const meta = {
  id: 'chat.question-answered-in-terminal-card-goes',
  title: 'The question card disappears when the question is answered in the terminal',
  area: 'chat',
  tags: ['questions'],
  edge: true,
};

export async function run(t) {
  t.fake.script(
    turnScript([think(300), hold('Which flavour?\n> 1. Alpha\n  2. Beta')], { on_prompt: 'go' }) +
      `
[[turn]]
steps = [ { do = "lines", count = 14, template = "work line {n}" }, ${say('all done')} ]
`,
  );
  const pane = await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'go');
  await t.expect.text('Agent is asking');

  await t.step('Answer in the terminal, not on the card');
  await pane.key('Enter');
  await t.fake.waitEvent('turn_done');

  await t.step('The agent carries on and the question scrolls away');
  await t.page.type('next', { into: COMPOSER });
  await t.press('Enter');
  await t.expect.text('all done', { timeout: 20000 });

  await t.step('The card is gone');
  await t.expect.notVisible('text=Agent is asking', { timeout: 10000 });
  await t.expect.notVisible('exact=Alpha');
}
