// The search box in Settings finds rows on any page; text that matches nothing says so; Esc clears the box.
import { calm, openSettings, settingsTitle } from './_area.mjs';

export const meta = {
  id: 'settings.search-rows',
  title: 'Searching Settings finds rows across pages and says so when nothing matches',
  area: 'settings',
  tags: ['search', 'keyboard'],
};

export async function run(t) {
  await calm(t);
  await openSettings(t);

  await t.step('Search for "theme"');
  await t.fill('aria=Search settings', 'theme');
  await t.expect.eventually(async () => (await settingsTitle(t)) === 'Search results', { message: 'the page title is Search results' });
  await t.expect.visible('exact=Theme', { within: 'css=.st-results' });
  await t.expect.text('Dark or light.');
  await t.expect.noText('Preload agents');

  await t.step('Search for a row on another page: "bridge address"');
  await t.fill('aria=Search settings', 'bridge address');
  await t.expect.text('Where other tools reach Puppet Master on this computer.');
  await t.expect.noText('Dark or light.');

  await t.step('Search for something that does not exist');
  await t.fill('aria=Search settings', 'qqzzxxnothing');
  await t.expect.text('No settings match');
  await t.expect.text('qqzzxxnothing');

  await t.step('Special characters are fine');
  await t.fill('aria=Search settings', '日本語 ✓ <*([');
  await t.expect.text('No settings match');

  await t.step('Esc clears the box and the page comes back');
  await t.press('Escape');
  await t.expect.value('aria=Search settings', '');
  await t.expect.eventually(async () => (await settingsTitle(t)) !== 'Search results', { message: 'a normal page is open again' });
}
