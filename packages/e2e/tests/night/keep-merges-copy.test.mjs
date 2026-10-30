// Keep takes two presses (it asks "Merge into ...?" first) and then merges the task's copy into the project.
import { sclick, openMorning, readFile, startOneTaskNight, writerScript, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.keep-merges-copy',
  title: 'Keep asks once more, then merges the task into the project',
  area: 'night',
  tags: ['night'],
  timeout: 90000,
};

const ROW = 'css=.ck-morning-task';

export async function run(t) {
  t.fake.script(writerScript('night.txt', 'kept from the night'));
  const { dir } = await startOneTaskNight(t);
  await t.waitFor('exact=Open Morning', { timeout: 60000 });
  await openMorning(t);

  await t.step('First press: it only asks');
  await sclick(t, 'exact=Keep', { within: ROW });
  await t.expect.text(/Merge into \S+\?/);
  t.expect.equal(readFile(dir, 'night.txt'), null, 'nothing is merged after the first press');

  await t.step('Second press: it merges');
  await sclick(t, 'exact=Keep', { within: ROW });
  await t.expect.eventually(() => readFile(dir, 'night.txt') === 'kept from the night', { message: 'the file reaches the project' });

  await t.step('The row is closed and says it was kept');
  await t.expect.notVisible('exact=Keep', { within: ROW });
  await t.expect.text(/Kept/i);
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
