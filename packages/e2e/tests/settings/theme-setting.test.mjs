// Settings > General > Theme: Dark and Light. It saves at once, the cockpit follows, and the sidebar button agrees.
import { calm, openSettings, themeNow, restoreDark, chooseOption, chosenOption } from './_area.mjs';

export const meta = {
  id: 'settings.theme-setting',
  title: 'The Theme setting switches the whole cockpit and agrees with the sidebar button',
  area: 'settings',
  tags: ['theme'],
};

export async function run(t) {
  await calm(t);
  try {
    await openSettings(t, 'General');

    await t.step('Theme: pick Light');
    await chooseOption(t, 'Theme', 'Light');
    await t.expect.eventually(async () => (await themeNow(t)) === 'light', { message: 'the cockpit is light' });
    await t.expect.visible('Switch to dark');

    await t.step('Reload the window: Light is still on');
    await t.page.reload();
    await t.expect.text('Puppet Master');
    await t.expect.eventually(async () => (await themeNow(t)) === 'light', { message: 'light after the reload' });

    await t.step('The sidebar button switches back, Settings shows Dark');
    await t.click('aria=Toggle theme');
    await t.expect.eventually(async () => (await themeNow(t)) === 'dark', { message: 'dark again' });
    await openSettings(t, 'General');
    t.expect.equal(await chosenOption(t, 'Theme'), 'Dark', 'Settings shows Dark');
  } finally {
    await restoreDark(t);
  }
}
