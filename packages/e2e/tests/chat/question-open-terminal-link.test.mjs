// "Open Terminal" on the card is the quiet way out: it shows the real terminal where the question is drawn.
import { turnScript, think, hold, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-open-terminal-link',
  title: 'Open Terminal on the question card switches to the Terminal tab',
  area: 'chat',
  tags: ['questions'],
  edge: true,
};

export async function run(t) {
  t.fake.script(turnScript([think(300), hold('Which flavour?\n> 1. Alpha\n  2. Beta')]));
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'go');
  await t.expect.text('Agent is asking');

  await t.step('Click Open Terminal');
  await t.click('exact=Open Terminal');

  await t.step('The terminal shows the same question and the card is gone');
  await t.expect.notVisible('aria=Message to the agent');
  await t.expect.eventually(async () => (await t.page.eval(`[...document.querySelectorAll('.xterm-rows')].map((e) => e.innerText).join('')`)).includes('Which flavour?'), {
    message: 'the terminal shows the menu',
  });
  await t.expect.notVisible('exact=Open Terminal');
}
