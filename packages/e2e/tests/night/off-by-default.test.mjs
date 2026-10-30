// Night Shift is a Labs feature: with its switch off nothing about it shows (no menu item, no palette row, no chip).
import { openNewMenu , resetNight, setSettings } from './_area.mjs';

export const meta = {
  id: 'night.off-by-default',
  title: 'With the Labs switch off, Night Shift shows nowhere',
  area: 'night',
  tags: ['night', 'labs'],
  edge: true,
};

export async function run(t) {
  await t.step('Open the plus menu');
  await openNewMenu(t);
  await t.expect.visible('More agents…');

  await t.step('The menu has no item for the night');
  await t.expect.notVisible('Tonight…');
  await t.press('Escape');

  await t.step('The command palette has no Night Shift row either');
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
  await t.type('night');
  await t.expect.noText('Night Shift: plan tonight');
  await t.press('Escape');

  await t.step('No Night Shift chip in the sidebar');
  await t.expect.noText('Open Morning');
}

// Start from the default: the Labs switch off.
export async function setup(t) {
  await resetNight(t);
  await setSettings(t, { night_shift_enabled: false });
}
