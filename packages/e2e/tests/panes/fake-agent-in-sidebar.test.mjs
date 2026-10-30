// A fake agent pane started through the bridge shows up in the sidebar and its terminal shows the agent.
import { PANE_ROW, selectPane } from '../_helpers.mjs';

export const meta = {
  id: 'panes.fake-agent-in-sidebar',
  title: 'A new agent pane appears in the sidebar and shows its terminal',
  area: 'panes',
  tags: ['smoke'],
};

export async function run(t) {
  await t.step('Start a fake agent');
  const pane = await t.spawnPane('nimbus');

  await t.step('It is listed in the sidebar');
  await t.expect.count(PANE_ROW, 1);
  await t.expect.noText('No agents running.');

  await t.step('Open it: the terminal shows the agent');
  await selectPane(t);
  await t.expect.text('NIMBUS 0.4.2');
  await t.expect.text('Type a task...');
  await t.expect.text('Idle');

  t.expect.equal((await pane.info()).status, 'idle', 'the app reports the pane as idle');
}
