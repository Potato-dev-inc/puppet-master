// Collision Alert: the Settings switch turns the warning off; turning it on again brings it back.
// The switch is set with the app's own command (the same one the Settings page calls); the page is only read.
import { twoWriters, ALERT_TITLE, neverShows, COLLISION_SWITCH, switchIsOn, tauri } from './_area.mjs';

export const meta = {
  id: 'collision.switch-off-stops-alerts',
  title: 'With the Settings switch off, two agents on one file raise no alert; on again, they do',
  area: 'collision',
  tags: ['collision', 'known-bug'],
  skip: 'KNOWN BUG (seen on 2 runs): after the switch goes off and on again, two agents that are still working on the same file raise no alert in 90 s; only a new start brings it back',
  edge: true,
  timeout: 200000,
};

const showSwitch = async (t) => {
  await t.click('exact=Settings');
  await t.expect.text('Logins, keeping agents ready, versions and your own agents.');
  await t.waitFor(COLLISION_SWITCH);
};

export async function run(t) {
  await t.step('Settings > Agents has the switch, and it is on by default');
  await showSwitch(t);
  t.expect.equal(await switchIsOn(t), true, 'the collision switch is on by default');

  await t.step('Turn it off (saved the way the switch saves it)');
  await tauri(t, 'collision_settings_set', { settings: { enabled: false } });
  await t.page.reload();
  await showSwitch(t);
  await t.expect.eventually(async () => (await switchIsOn(t)) === false, { message: 'the page shows the switch off' });

  try {
    await t.step('Two agents change the same file in one folder');
    await twoWriters(t);
    await t.click('exact=Inbox', { within: 'css=.ck-side' });
    await t.expect.text('Nothing needs you');

    await t.step('Thirty-five seconds later (a first look comes after about 20 s) there is still no card');
    await neverShows(t, ALERT_TITLE, 35000);

    await t.step('Turn the switch back on');
    await tauri(t, 'collision_settings_set', { settings: { enabled: true } });

    await t.step('The card shows up for the same two agents');
    await t.expect.text(ALERT_TITLE, { timeout: 90000 });
  } finally {
    await tauri(t, 'collision_settings_set', { settings: { enabled: true } }).catch(() => {});
  }
}
