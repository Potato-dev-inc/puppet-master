// Edge case: in Individual mode the app's main window is Settings, so a second start opens the Settings window.
import { calm, cdpTargets, launchAgain, restoreOrchestration } from './_area.mjs';

export const meta = {
  id: 'app.second-launch-in-individual-opens-settings',
  title: 'In Individual mode starting the app again opens the Settings window',
  area: 'app',
  tags: ['single-instance', 'mode'],
  edge: true,
};

const settingsWindows = async (t) => (await cdpTargets(t)).filter((x) => x.type === 'page' && /[?&]settings=1/.test(x.url));

export async function run(t) {
  await calm(t);
  try {
    await t.step('Switch to Individual mode');
    await t.click('exact=Individual');
    await t.expect.text('Panes are in their own windows.');
    const expected = Math.max(1, (await settingsWindows(t)).length); // a Settings window left by an earlier test is shown again

    await t.step('Start the app a second time');
    const second = launchAgain(t);
    t.expect.equal(second.status, 0, 'the second process quit with code 0');

    await t.step('The running app opens its Settings window');
    await t.expect.eventually(async () => (await settingsWindows(t)).length === expected, { message: 'one Settings window opened', timeout: 20000 });
  } finally {
    await restoreOrchestration(t);
  }
}
