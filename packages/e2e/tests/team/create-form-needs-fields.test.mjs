// "Create a team" cannot be pressed until it has a team name, your name and a place to meet (a relay address or a shared
// folder). Nothing is pressed here: making a team writes keys to the system's credential store.
import { guarded, openGeneralSettings, reveal } from '../companion/_area.mjs';

export const meta = {
  id: 'team.create-form-needs-fields',
  title: 'Create team stays off until the team name, your name and a relay or folder are filled in',
  area: 'team',
  tags: ['needs-external'],
  edge: true,
};

const off = (t) => t.page.eval(`[...document.querySelectorAll('.st-team button')].find(b => b.textContent === 'Create team').disabled`);
const TEAM = (placeholder) => `css=.st-team input[placeholder="${placeholder}"]`;

export const run = guarded(async (t) => {
  await openGeneralSettings(t);
  await reveal(t, '.st-team');
  await reveal(t, '.st-team-section .st-team-h');

  await t.step('With nothing typed the button is off');
  t.expect.equal(await off(t), true);

  await t.step('A team name and your name are not enough');
  await t.page.fill(TEAM('Acme'), 'Night Owls');
  await t.page.fill(TEAM('Maya'), 'Ren');
  t.expect.equal(await off(t), true, 'still off without a relay or folder');

  await t.step('A relay address makes it ready');
  await t.page.fill(TEAM('https://relay.example.com'), 'https://relay.invalid');
  t.expect.equal(await off(t), false);

  await t.step('Switching to a shared folder swaps the field and needs a folder');
  await t.click('Use a shared folder instead', { within: 'css=.st-team-inline' });
  await t.expect.visible('Shared folder');
  t.expect.equal(await off(t), true, 'the relay address no longer counts');
  await t.page.fill('css=.st-team input[placeholder^="D:"]', 'D:\\shared\\wire');
  t.expect.equal(await off(t), false);

  await t.step('A blank team name takes it away again');
  await t.page.fill(TEAM('Acme'), '   ');
  t.expect.equal(await off(t), true);
});
