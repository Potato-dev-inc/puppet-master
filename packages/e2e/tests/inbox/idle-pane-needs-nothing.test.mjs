// A running agent that does not ask anything is not in the Inbox.
import { openInbox } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.idle-pane-needs-nothing',
  title: 'An idle agent adds nothing to the Inbox',
  area: 'inbox',
  tags: ['inbox'],
  edge: true,
};

export async function run(t) {
  await t.spawnPane('nimbus');
  await t.step('Open the Inbox with one idle agent');
  await openInbox(t);
  await t.expect.text('Nothing needs you');
  await t.expect.noText('Waiting for you');
  t.expect.equal((await t.panes()).length, 1, 'the agent is still running');
}
