// The sun/moon button at the bottom of the sidebar switches dark and light; the choice survives a reload of the window.
import { themeNow, calm, restoreDark } from './_area.mjs';

export const meta = {
  id: 'app.theme-toggle',
  title: 'The theme button switches dark and light and the choice survives a reload',
  area: 'app',
  tags: ['smoke', 'theme'],
};

export async function run(t) {
  await calm(t);
  try {
    await t.step('The cockpit starts dark');
    t.expect.equal(await themeNow(t), 'dark', 'dark at start');
    await t.expect.visible('aria=Toggle theme');
    await t.expect.visible('Switch to light');

    await t.step('Click the theme button: light');
    await t.click('aria=Toggle theme');
    await t.expect.eventually(async () => (await themeNow(t)) === 'light', { message: 'the cockpit is light' });
    await t.expect.visible('Switch to dark');

    await t.step('Reload the window: still light (it was saved)');
    await t.page.reload();
    await t.expect.text('Puppet Master');
    await t.expect.eventually(async () => (await themeNow(t)) === 'light', { message: 'light after the reload' });

    await t.step('Click again: dark');
    await t.click('aria=Toggle theme');
    await t.expect.eventually(async () => (await themeNow(t)) === 'dark', { message: 'dark again' });
  } finally {
    await restoreDark(t); // the theme is a saved setting: leave it as it was
  }
}
