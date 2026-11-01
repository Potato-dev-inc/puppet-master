// A chat whose agent crashes in the middle of a turn: the chat says "The chat stopped: ..." with a Restart chat button
// (no spinner is left), and Restart chat brings the same conversation back so the next message works.
import path from 'node:path';
import { say, MESSAGE_BOX } from './_area.mjs';
import { startNimbusChat, chatState } from './_robust.mjs';

export const meta = {
  id: 'acp.chat-crash-restart',
  title: 'The agent crashes mid-turn: the chat says why and Restart chat brings it back',
  area: 'acp',
  tags: ['acp', 'labs', 'robust'],
  fresh: true,
  timeout: 180000,
};

const shot = (t, name) => t.page.screenshot(path.join(t.outDir, `${name}.png`));

export async function run(t) {
  const pane = await startNimbusChat(t);

  await t.step('A first message works');
  await say(t, 'stream 3');
  await t.expect.text('w2');

  await t.step('The agent crashes in the middle of a turn');
  await say(t, 'crash');
  await t.waitFor('css=[data-testid=acp-stopped]');
  await t.expect.text('The chat stopped: the agent stopped unexpectedly (exit code 3)');
  await t.expect.eventually(async () => !(await t.page.has('css=[data-testid=acp-strip]')), { message: 'no Working strip is left' });
  t.expect.equal((await chatState(t, pane.id)).turnInFlight, false, 'nothing is still running');
  await shot(t, '1-chat-stopped');

  await t.step('Restart chat brings the conversation back');
  await t.click('exact=Restart chat');
  await t.expect.eventually(async () => !(await t.page.has('css=[data-testid=acp-stopped]')), { timeout: 30000, message: 'the stopped line is gone' });
  await t.waitFor(MESSAGE_BOX);
  await say(t, 'stream 2');
  await t.expect.text('w1');
  await shot(t, '2-chat-restarted');
}
