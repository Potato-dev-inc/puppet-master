// Throw away takes two presses ("Delete this copy?") and leaves the project exactly as it was.
import { sclick, openMorning, readFile, startOneTaskNight, writerScript, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.throw-away-copy',
  title: 'Throw away asks once more, then drops the copy and keeps the project untouched',
  area: 'night',
  tags: ['night'],
  timeout: 90000,
};

const ROW = 'css=.ck-morning-task';

export async function run(t) {
  t.fake.script(writerScript('night.txt', 'not wanted'));
  const { dir } = await startOneTaskNight(t);
  await t.waitFor('exact=Open Morning', { timeout: 60000 });
  await openMorning(t);

  await t.step('First press: it asks to delete the copy');
  await sclick(t, 'exact=Throw away', { within: ROW });
  await t.expect.text('Delete this copy?');

  await t.step('Second press: the copy is gone');
  await sclick(t, 'exact=Throw away', { within: ROW });
  await t.expect.notVisible('exact=Keep', { within: ROW });
  await t.expect.notVisible('exact=Throw away', { within: ROW });

  await t.step('The project has no trace of the task');
  t.expect.equal(readFile(dir, 'night.txt'), null);
  await t.expect.text('write the night file');
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
