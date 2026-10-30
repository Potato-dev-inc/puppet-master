// A pane that has not done anything yet: the Receipt and History tabs say what they will hold.
import { openChat } from './_area.mjs';

export const meta = {
  id: 'chat.tabs-receipt-and-history-empty',
  title: 'Receipt and History of a fresh pane say what will appear there',
  area: 'chat',
  tags: ['tabs'],
  edge: true,
};

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: t.project('tabs-empty') });
  await openChat(t);

  await t.step('Receipt');
  await t.click('exact=Receipt');
  await t.expect.text('No turns yet');
  await t.expect.text('The receipt appears when Nimbus finishes its first turn.');

  await t.step('History');
  await t.click('exact=History');
  await t.expect.text('No checkpoints yet');
  await t.expect.text('Your files are saved automatically when this agent starts and finishes a turn.');
}
