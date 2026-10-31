// Output stress: the agent prints a clickable link (OSC 8). The words show without the raw escape codes in the terminal text and in Chat.
import { calm, S, oneTurn, openChat, sendChat, waitStatus } from './_area.mjs';

export const meta = {
  id: 'work.output-link-in-reply',
  title: 'A link in the reply shows as its words, with no escape codes, in the terminal and in Chat',
  area: 'work',
  tags: ['work', 'output'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  t.fake.script(oneTurn([S.think(300), S.link('the build report', 'file:///C:/build/report.html', 'Open ', ' when ready'), S.say('link printed')]));
  const pane = await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'where is the report');
  await t.fake.waitEvent('turn_done');
  await waitStatus(t, pane, 'idle', 30000);

  await t.step('The terminal text reads "Open the build report when ready" and has no escape sequence');
  const screen = await pane.screen();
  t.expect.includes(screen, 'Open the build report when ready');
  t.expect.ok(!screen.includes(']8;'), 'no raw OSC 8 text in the screen');
  t.expect.ok(!screen.includes('\u001b'), 'no raw escape character in the screen text');

  await t.step('Chat shows the same words and no escape codes');
  await t.expect.text('the build report');
  await t.expect.noText(']8;');
  await t.expect.noText('report.html');
}
