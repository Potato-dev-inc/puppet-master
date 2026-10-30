// Settings opens inside the cockpit on the Agents page and lists the sections.
import { calm } from './_area.mjs';

export const meta = {
  id: 'settings.opens',
  title: 'Settings opens on the Agents page with all its sections',
  area: 'settings',
  tags: ['smoke'],
};

export async function run(t) {
  await calm(t);
  await t.step('Open Settings');
  await t.click('exact=Settings');

  await t.step('The sections are listed');
  for (const name of ['General', 'Agents', 'Orchestrator & Models', 'Keys', 'Bridge & MCP', 'Phone']) await t.expect.visible(`exact=${name}`);

  await t.step('The Agents page is open');
  await t.expect.text('Logins, keeping agents ready, versions and your own agents.');

  await t.step('Open another section');
  await t.click('exact=General');
  await t.expect.noText('Logins, keeping agents ready, versions and your own agents.');
}
