// Settings opens inside the cockpit. The Back button and Esc both return to what was open before (here: a pane).
import { calm, openPane, openSettings, PANE_TITLE, paneHeading } from './_area.mjs';

export const meta = {
  id: 'settings.back-and-escape',
  title: 'Back and Esc leave Settings and return to the pane that was open',
  area: 'settings',
  tags: ['keyboard'],
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');

  await t.step('Open Settings from a pane');
  await openSettings(t);
  await t.expect.notVisible(PANE_TITLE);

  await t.step('The Back button returns to the pane');
  await t.click('aria=Back');
  await t.waitFor(PANE_TITLE);
  t.expect.equal(await paneHeading(t), 'Nimbus', 'the pane is open again');
  await t.expect.text('NIMBUS 0.4.2');

  await t.step('Open Settings again and press Esc');
  await openSettings(t);
  await t.press('Escape');
  await t.waitFor(PANE_TITLE);
  await t.expect.notVisible('css=.settings--inline');

  await t.step('Esc inside the search box only clears it (edge): Settings stays open');
  await openSettings(t);
  await t.fill('aria=Search settings', 'theme');
  await t.press('Escape');
  await t.expect.value('aria=Search settings', '');
  await t.expect.visible('css=.settings--inline');
  await t.press('Escape');
  await t.expect.notVisible('css=.settings--inline');
}
