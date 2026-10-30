// Two panes at once: only the pane that asks shows the card. The other pane's Chat stays quiet.
import { APPROVE_FILE, installAsker, openChat, sendChat, selectPane } from './_area.mjs';

export const meta = {
  id: 'chat.question-card-only-in-asking-pane',
  title: 'The question card shows only in the chat of the pane that asks',
  area: 'chat',
  tags: ['questions', 'two-panes'],
  edge: true,
};

export async function run(t) {
  await installAsker(t);
  t.fake.script(APPROVE_FILE);
  await t.spawnPane('asker');
  t.fake.script('boot_ms = 100\ninline = true\n');
  await t.spawnPane('nimbus');

  await t.step('Ask the first agent to make a file: it stops on a question');
  await openChat(t, 'asker');
  await sendChat(t, 'make the file');
  await t.expect.text('Answer the agent');

  await t.step('The other pane shows no card');
  await openChat(t, 'Nimbus');
  await t.expect.noText('Answer the agent');
  await t.expect.notVisible('exact=Allow once');

  await t.step('Back in the first pane the card is still waiting');
  await openChat(t, 'make the file');
  await t.expect.visible('exact=Allow once');
  await t.click('exact=Deny');
  await t.expect.text('nothing written');
}
