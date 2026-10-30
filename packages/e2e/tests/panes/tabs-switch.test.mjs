// A pane has five tabs. Each one opens its own page; a new agent has nothing to show on the last four, and says so.
import { calm, openPane, selectedTab } from './_area.mjs';

export const meta = {
  id: 'panes.tabs-switch',
  title: 'The Terminal, Chat, Transcript, Receipt and History tabs each open their page',
  area: 'panes',
  tags: ['smoke', 'tabs'],
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');

  await t.step('Terminal is the tab that opens first');
  t.expect.equal(await selectedTab(t), 'Terminal');
  await t.expect.text('NIMBUS 0.4.2');
  for (const tab of ['Terminal', 'Chat', 'Transcript', 'Receipt', 'History']) await t.expect.visible(`exact=${tab}`);

  await t.step('Chat: waits for the first message');
  await t.click('exact=Chat');
  t.expect.equal(await selectedTab(t), 'Chat');
  await t.waitFor('aria=Message to the agent');
  await t.expect.text('Waiting for a message on the terminal');

  await t.step('Transcript: nothing handed over yet');
  await t.click('exact=Transcript');
  t.expect.equal(await selectedTab(t), 'Transcript');
  await t.expect.text('No transcript');
  await t.expect.text('What you typed here yourself is on the Terminal tab.');

  await t.step('Receipt: no turn finished yet');
  await t.click('exact=Receipt');
  await t.expect.text('No turns yet');
  await t.expect.text('The receipt appears when Nimbus finishes its first turn.');

  await t.step('History: no checkpoints yet');
  await t.click('exact=History');
  await t.expect.text('No checkpoints yet');

  await t.step('Back to Terminal: the agent is still there');
  await t.click('exact=Terminal');
  await t.expect.text('NIMBUS 0.4.2');
  await t.expect.text('Type a task...');
}
