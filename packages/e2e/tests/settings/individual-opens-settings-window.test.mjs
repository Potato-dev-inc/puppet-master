// In Individual mode Settings is its own window, not a page of the cockpit. Opening it twice does not make two windows.
import { calm, cdpTargets, restoreOrchestration } from './_area.mjs';

export const meta = {
  id: 'settings.individual-opens-settings-window',
  title: 'In Individual mode the Settings button opens one separate Settings window',
  area: 'settings',
  tags: ['mode'],
  edge: true,
};

const settingsWindows = async (t) => (await cdpTargets(t)).filter((x) => x.type === 'page' && /[?&]settings=1/.test(x.url));

export async function run(t) {
  await calm(t);
  try {
    await t.step('Switch to Individual mode');
    await t.click('exact=Individual');
    await t.expect.text('Panes are in their own windows.');
    // An earlier test may have left the Settings window open (it stays until the app quits): then it is shown again.
    const expected = Math.max(1, (await settingsWindows(t)).length);
    t.expect.ok(expected === 1, 'at most one Settings window exists before the click');

    await t.step('Click Settings');
    await t.click('exact=Settings', { within: 'css=.ck-side' });
    await t.expect.eventually(async () => (await settingsWindows(t)).length === expected, { message: async () => 'one Settings window opened; the windows are ' + JSON.stringify((await cdpTargets(t)).map((x) => x.url.split(String.fromCharCode(63))[1])), timeout: 15000 });
    await t.expect.notVisible('css=.settings--inline');
    await t.expect.text('Panes are in their own windows.');

    await t.step('Click Settings again: still one window');
    await t.click('exact=Settings', { within: 'css=.ck-side' });
    await t.expect.eventually(async () => (await settingsWindows(t)).length === expected, { message: 'still one Settings window' });
  } finally {
    await restoreOrchestration(t);
  }
}
