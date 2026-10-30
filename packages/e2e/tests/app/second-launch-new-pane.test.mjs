// Edge case: the second start can carry --new-pane <agent>; the running app opens a pane of that agent. A name the app does not
// know opens nothing and breaks nothing.
import { calm, launchAgain } from './_area.mjs';

export const meta = {
  id: 'app.second-launch-new-pane',
  title: 'Starting the app with --new-pane nimbus opens a Nimbus pane in the running app',
  area: 'app',
  tags: ['single-instance'],
  edge: true,
};

export async function run(t) {
  await calm(t);

  await t.step('Start the app again with --new-pane nimbus');
  const second = launchAgain(t, ['--new-pane', 'nimbus']);
  t.expect.equal(second.status, 0, 'the second process quit with code 0');

  await t.step('A Nimbus pane appears in the running app');
  await t.expect.eventually(async () => (await t.panes()).filter((p) => p.agent_type === 'nimbus').length === 1, { message: 'one Nimbus pane', timeout: 20000 });
  await t.expect.text('Nimbus');

  await t.step('An unknown agent name opens nothing');
  const before = (await t.panes()).length;
  launchAgain(t, ['--new-pane', 'no-such-agent']);
  await t.expect.eventually(async () => (await t.panes()).length === before, { message: 'no pane was added' });
  await t.expect.text('Puppet Master');
}
