// Labs > Team: Create team and Ask to join stay off until their fields are filled; nothing is sent (the test never presses them).
// needs-external for a real relay or shared folder.
import { labsOff, openSettings, scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'labs.team-forms-need-input',
  title: 'The Team block keeps Create team and Ask to join off until the fields are filled',
  area: 'labs',
  tags: ['labs', 'needs-external'],
  edge: true,
};

const off = (t, label) => t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === ${JSON.stringify(label)}).disabled`);

export async function run(t) {
  await openSettings(t, 'General');
  await t.waitFor('css=input[placeholder="Acme"]');
  await scrollToCss(t, 'input[placeholder="Acme"]');

  await t.step('Empty forms: both buttons are off');
  t.expect.equal(await off(t, 'Create team'), true);
  t.expect.equal(await off(t, 'Ask to join'), true);

  await t.step('Fill two of the three create fields: still off');
  await t.fill('css=input[placeholder="Acme"]', 'Example team');
  await t.fill('css=input[placeholder="Maya"]', 'Alex');
  t.expect.equal(await off(t, 'Create team'), true);

  await t.step('Add a relay address: Create team turns on (it is not pressed)');
  await t.fill('css=input[placeholder="https://relay.example.com"]', 'https://relay.invalid');
  await t.expect.eventually(async () => (await off(t, 'Create team')) === false, { message: 'Create team is enabled' });

  await t.step('Use a shared folder instead: the relay field is replaced and the button waits for a folder');
  await t.click('aria=Use a shared folder instead');
  await t.waitFor('css=input[placeholder^="D:"]');
  t.expect.equal(await off(t, 'Create team'), true);
  await t.expect.noText('Host your own relay on Cloudflare or with Docker.');

  await t.step('Clear the fields so nothing is left typed');
  await t.fill('css=input[placeholder="Acme"]', '');
  await t.fill('css=input[placeholder="Maya"]', '');
  t.expect.equal(await t.bridge.get('/health').then((r) => r.status), 200);
}

export const setup = labsOff;
