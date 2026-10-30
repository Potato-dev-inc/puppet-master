// Settings > General > Labs > Night Shift: turning it on adds "Tonight…" to the plus menu and the palette; off removes them.
import { flip, isOn, openNewMenu, openSettings , resetNight, setSettings } from './_area.mjs';

export const meta = {
  id: 'night.labs-switch',
  title: 'The Labs switch adds and removes Tonight… in the plus menu',
  area: 'night',
  tags: ['night', 'labs'],
};

export async function run(t) {
  try {
    await body(t);
  } finally {
    // The Settings window keeps its own copy of the settings; carry the welcome flag along so the next test starts normally.
    await setSettings(t, { night_shift_enabled: false });
  }
}

async function body(t) {
  await t.step('Open Settings > General');
  await openSettings(t, 'General');
  await t.expect.text('Agents work through your tasks overnight, in separate copies.');
  t.expect.equal(await isOn(t, 'Night Shift'), false, 'the switch starts off');

  await t.step('Turn Night Shift on');
  await flip(t, 'Night Shift');
  await t.expect.eventually(() => isOn(t, 'Night Shift'), { message: 'the switch turns on' });

  await t.step('Go back to the cockpit: the plus menu now has Tonight…');
  await t.click('exact=Inbox');
  await openNewMenu(t);
  await t.expect.visible('exact=Tonight…');
  await t.press('Escape');

  await t.step('The palette offers the Night Shift rows');
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
  await t.type('night');
  await t.expect.text('Night Shift: plan tonight');
  await t.press('Escape');

  await t.step('Turn it off again');
  await openSettings(t, 'General');
  await flip(t, 'Night Shift');
  await t.expect.eventually(async () => !(await isOn(t, 'Night Shift')), { message: 'the switch turns off' });
  await t.click('exact=Inbox');
  await openNewMenu(t);
  await t.expect.notVisible('exact=Tonight…');
}

// Start from the default: the Labs switch off.
export async function setup(t) {
  await resetNight(t);
  await setSettings(t, { night_shift_enabled: false });
}
