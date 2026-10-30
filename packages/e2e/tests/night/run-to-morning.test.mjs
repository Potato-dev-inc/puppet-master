// A whole night with a fake agent: plan one task, start it, watch the chip, read the Morning page.
import { openMorning, readFile, startOneTaskNight, writerScript, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.run-to-morning',
  title: 'A one-task night runs on a fake agent and the Morning page shows the result',
  area: 'night',
  tags: ['night', 'smoke'],
  timeout: 90000,
};

export async function run(t) {
  t.fake.script(writerScript('night.txt', 'done by nimbus'));

  await t.step('Plan one task on Nimbus and start the night');
  const { dir, stopAt } = await startOneTaskNight(t);

  await t.step('The chip says the night is going, then that it is done');
  await t.expect.text('Night Shift');
  await t.waitFor('exact=Open Morning', { timeout: 60000 });
  await t.expect.text('1 of 1 done');

  await t.step('Open the Morning page');
  await openMorning(t);
  await t.expect.text('1 of 1 done, no tests ran');
  await t.expect.text('READY TO KEEP (1)');
  await t.expect.text('write the night file');
  await t.expect.text('Done. No tests declared, nothing was checked.');
  await t.expect.text('+1 -0 · 1 file');

  await t.step('The night log tells what happened');
  await t.expect.text(`Night Shift started. It stops at ${stopAt}.`);
  await t.expect.text('Task 1 started on Nimbus.');
  await t.expect.text('Finished. 1 of 1 done, no tests ran');

  await t.step('The project itself was not touched: the work is in a separate copy');
  t.expect.equal(readFile(dir, 'night.txt'), null, 'the file is not in the project yet');
  await t.expect.visible('exact=Keep');
  await t.expect.visible('exact=Throw away');
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
