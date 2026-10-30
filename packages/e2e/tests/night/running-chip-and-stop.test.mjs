// While a night runs the chip shows how far it is; Stop asks once more, then ends the night and keeps the copies.
import { openMorning, slowScript, startOneTaskNight, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.running-chip-and-stop',
  title: 'The chip shows a running night and Stop Night Shift ends it after a confirm',
  area: 'night',
  tags: ['night'],
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
  const { stopAt } = await startOneTaskNight(t, { text: 'a very long task' });

  await t.step('The chip shows the night is running');
  await t.waitFor('css=[data-testid=night-chip]');
  await t.expect.text(`0 of 1 until ${stopAt}`);
  await t.expect.visible('exact=Stop');

  await t.step('Open the chip: the task is listed');
  await t.click('css=.ck-night-chip-main');
  await t.waitFor('css=[data-testid=night-popover]');
  await t.expect.visible({ text: 'a very long task', within: 'css=[data-testid=night-popover]' });

  await t.step('Stop asks before it stops anything');
  await t.click('exact=Stop Night Shift');
  await t.expect.text('Stops everything now. Copies are kept.');

  await t.step('Confirm: the night ends');
  await t.click('exact=Stop Night Shift');
  await t.waitFor('exact=Open Morning');

  await t.step('The Morning page says the night was stopped');
  await openMorning(t);
  await t.expect.text('Morning');
  await t.expect.noText('Running. Each task shows here as it finishes.');
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
