// Starting the app a second time does not start a second app: the running one shows its main window and the new process quits.
import { calm, cdpTargets, launchAgain, openPane, PANE_TITLE } from './_area.mjs';

export const meta = {
  id: 'app.second-launch-shows-running-app',
  title: 'Starting the app again shows the running app and starts no second one',
  area: 'app',
  tags: ['single-instance'],
};

const cockpits = async (t) => (await cdpTargets(t)).filter((x) => x.type === 'page' && /[?&]cockpit=1/.test(x.url));

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  t.expect.equal((await cockpits(t)).length, 1, 'one cockpit window');

  await t.step('Start the app a second time');
  const second = launchAgain(t);
  t.log('second launch', second);
  t.expect.equal(second.error, null, 'the second process could be started');
  t.expect.equal(second.status, 0, 'the second process quit by itself with code 0');

  await t.step('The running app is still the only one: same window, same pane');
  t.expect.equal((await cockpits(t)).length, 1, 'still one cockpit window');
  t.expect.equal((await t.panes()).length, 1, 'one pane, no second app started');
  await t.waitFor(PANE_TITLE);
  await t.expect.text('NIMBUS 0.4.2');

  await t.step('The app still answers');
  await t.click('exact=History');
  await t.expect.text('No checkpoints yet');
}
