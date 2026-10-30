// Every page of Settings opens and says what it is for. (Keys is only listed here: its page shows the last digits of the
// person's real stored keys, which a recording should not show.)
import { calm, openSettings, goToPage, settingsTitle } from './_area.mjs';

export const meta = {
  id: 'settings.every-section',
  title: 'Each Settings page opens with its title and its one-line purpose',
  area: 'settings',
  tags: ['smoke'],
};

const PAGES = [
  ['General', 'Project, appearance, safety, early features and updates.', 'Default project folder'],
  ['Agents', 'Logins, keeping agents ready, versions and your own agents.', 'Preload agents'],
  ['Orchestrator & Models', 'Choose the AI that coordinates your other agents, and which models it can use.', 'Default orchestrator'],
  ['Bridge & MCP', 'How other tools and agents connect to Puppet Master.', 'Bridge address'],
  ['Phone', 'Pair a phone to watch and steer your agents.', 'Paired phones'],
];

export async function run(t) {
  await calm(t);
  await openSettings(t);

  await t.step('The pages are listed, Keys among them');
  for (const [name] of PAGES) await t.expect.visible(`exact=${name}`, { within: 'css=.st-nav' });
  await t.expect.visible('exact=Keys', { within: 'css=.st-nav' });
  await t.expect.visible('exact=Coming later', { within: 'css=.st-side-foot' });

  await t.step('Settings opens on the Agents page');
  t.expect.equal(await settingsTitle(t), 'Agents');

  for (const [name, purpose, sample] of PAGES) {
    await t.step(`Open ${name}`);
    await goToPage(t, name);
    await t.expect.text(purpose);
    await t.expect.text(sample);
  }
}
