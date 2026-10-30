// "Cancel" in the Stop confirm leaves the night running.
import { slowScript, startOneTaskNight, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.stop-cancelled',
  title: 'Cancelling the Stop question leaves the night running',
  area: 'night',
  tags: ['night'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  try {
    await body(t);
  } finally {
    await resetNight(t); // leave no night running for the next test
  }
}

async function body(t) {
  t.fake.script(slowScript(120000));
  const { stopAt } = await startOneTaskNight(t, { text: 'keep going all night' });
  await t.waitFor('css=[data-testid=night-chip]');
  await t.expect.text(`0 of 1 until ${stopAt}`);

  await t.step('Press Stop on the chip');
  await t.click('exact=Stop');
  await t.expect.text('Stops everything now. Copies are kept.');

  await t.step('Cancel');
  await t.click('exact=Cancel', { within: 'css=[data-testid=night-popover]' });
  await t.expect.noText('Stops everything now. Copies are kept.');

  await t.step('The night is still running');
  await t.expect.text(`0 of 1 until ${stopAt}`);
  await t.expect.noText('Open Morning');
  await t.expect.visible('exact=Stop');
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
