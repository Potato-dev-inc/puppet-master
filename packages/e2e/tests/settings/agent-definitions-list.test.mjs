// Settings > Agents > Definitions lists the agent definitions the app loaded: the fake agent Nimbus is Active and experimental.
import { calm, openSettings } from './_area.mjs';

export const meta = {
  id: 'settings.agent-definitions-list',
  title: 'The Definitions list shows Nimbus as an active experimental agent',
  area: 'settings',
  tags: ['agents', 'definitions'],
};

export async function run(t) {
  await calm(t);
  await openSettings(t, 'Agents');

  await t.step('The block explains that a definition runs a program');
  await t.expect.text('Add your own agents with a small settings file.');
  await t.expect.text('A definition runs a program, so a new or changed one stays off until you review and approve it here.');

  await t.step('Nimbus is listed with its source and state');
  await t.waitFor('css=[data-testid=definition-row]');
  const rows = await t.page.eval(`[...document.querySelectorAll('[data-testid=definition-row]')].map((r) => ({
    name: r.querySelector('span.text-sm')?.innerText.trim(),
    source: r.querySelector('[data-testid=source-badge]')?.innerText.trim(),
    state: r.querySelector('[data-testid=state-badge]')?.innerText.trim(),
    experimental: !!r.querySelector('[data-testid=experimental-badge]'),
  }))`);
  const nimbus = rows.find((r) => r.name === 'nimbus');
  t.expect.ok(nimbus, 'a row for nimbus: ' + JSON.stringify(rows));
  t.expect.equal([nimbus.source, nimbus.state, nimbus.experimental], ['User', 'Active', true], 'user definition, active, experimental');

  await t.step('An active definition can be checked, exported and revoked');
  for (const name of ['Check with fixtures', 'Export', 'Revoke']) await t.expect.visible(`exact=${name}`);
  await t.expect.notVisible('exact=Review and approve');

  await t.step('The toolbar of the block');
  for (const name of ['Reload', 'Import', 'Add with wizard', 'Browse more agents', 'Add as generic agent']) await t.expect.visible(`exact=${name}`);
}
