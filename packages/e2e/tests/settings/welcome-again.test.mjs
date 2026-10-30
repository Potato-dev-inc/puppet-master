// Settings > General > Show the welcome opens the first-run welcome again; Skip setup closes it without changing anything.
import { calm, clickControl, openSettings } from './_area.mjs';

export const meta = {
  id: 'settings.welcome-again',
  title: 'Show the welcome in Settings opens the welcome again and Skip setup closes it',
  area: 'settings',
  tags: ['first-run'],
};

export async function run(t) {
  await calm(t);
  await openSettings(t, 'General');
  await t.expect.noText('How do you work?');

  await t.step('Click Show the welcome');
  await clickControl(t, 'exact=Show the welcome');
  await t.expect.text('Welcome to Puppet Master');
  await t.expect.text('How do you work?');

  await t.step('Skip setup closes it');
  await t.click('Skip setup');
  await t.expect.noText('How do you work?');

  await t.step('The same welcome is in the palette, too');
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
  await t.type('welcome');
  await t.press('Enter');
  await t.expect.text('How do you work?');
  await t.click('Skip setup');
  await t.expect.noText('How do you work?');
}
