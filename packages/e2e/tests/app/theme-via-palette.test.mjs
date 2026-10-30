// The palette has a row that switches the theme, and its wording follows the theme that is on.
import { openPalette, paletteTitles, themeNow, calm, restoreDark } from './_area.mjs';

export const meta = {
  id: 'app.theme-via-palette',
  title: 'The palette row switches the theme and then offers the other one',
  area: 'app',
  tags: ['theme', 'palette'],
};

export async function run(t) {
  await calm(t);
  try {
    await t.step('In dark the palette offers the light theme');
    await openPalette(t);
    await t.type('theme');
    t.expect.includes(await paletteTitles(t), 'Switch to light theme');

    await t.step('Enter switches to light');
    await t.press('Enter');
    await t.expect.eventually(async () => (await themeNow(t)) === 'light', { message: 'the cockpit is light' });

    await t.step('Now the palette offers the dark theme');
    await openPalette(t);
    await t.type('theme');
    t.expect.includes(await paletteTitles(t), 'Switch to dark theme');
    await t.press('Enter');
    await t.expect.eventually(async () => (await themeNow(t)) === 'dark', { message: 'dark again' });
  } finally {
    await restoreDark(t);
  }
}
