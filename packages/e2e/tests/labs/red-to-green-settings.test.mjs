// Labs > Red to Green: off by default, with its limits and the promise that nothing is pushed, merged or re-run.
import { labsOff, openSettings, scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'labs.red-to-green-settings',
  title: 'Red to Green is off with no pull requests and states its limits and promises',
  area: 'labs',
  tags: ['labs', 'needs-external'],
};

const SWITCH = 'button[role=switch][aria-label="Offer to fix failed CI"]';

export async function run(t) {
  await openSettings(t, 'General');
  await t.waitFor(`css=${SWITCH}`);
  await scrollToCss(t, SWITCH);

  await t.step('The switch is off and explains what it would do');
  t.expect.equal(await t.page.eval(`document.querySelector('${SWITCH}').getAttribute('aria-checked')`), 'false');
  await t.expect.text('When CI fails on a pull request Puppet Master opened, you get a card to have the same agent fix it.');

  await t.step('The four limits are listed with their ranges');
  for (const [label, range] of [['Tries per pull request', '1 to 3'], ['Turns per try', '1 to 50'], ['Minutes per try', '5 to 60'], ['Fixes a day', '1 to 20']]) {
    await t.expect.text(label);
    await t.expect.text(range);
  }

  await t.step('Nothing is watched yet');
  await t.expect.text('No pull requests yet. Puppet Master watches one once it opens it.');

  await t.step('The promises about pushing, merging and re-running CI are stated');
  await t.expect.text('The agent can\'t push during a fix.');
  await t.expect.text('Puppet Master never re-runs CI, merges or force-pushes.');
}

export const setup = labsOff;
