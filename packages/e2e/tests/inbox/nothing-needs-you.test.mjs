// After an agent ran and was closed, the empty Inbox says nothing needs you.
import { openInbox } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.nothing-needs-you',
  title: 'The Inbox says nothing needs you once the agents are closed',
  area: 'inbox',
  tags: ['inbox', 'smoke'],
  edge: true,
};

export async function run(t) {
  const pane = await t.spawnPane('nimbus');
  await t.step('Close the only agent');
  await pane.kill();
  await t.expect.text('No agents running.');

  await t.step('The Inbox is calm');
  await openInbox(t);
  await t.expect.text('Nothing needs you');
  await t.expect.text('Agents that stop to ask a question will show up here.');
  await t.expect.visible('exact=Set up your agents');
  await t.expect.noText('things need you');
}
