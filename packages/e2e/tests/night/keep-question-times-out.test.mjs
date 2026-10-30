// One press of Keep only asks. Walk away and the question goes back to the buttons after a few seconds; nothing merges.
import { sclick, openMorning, readFile, startOneTaskNight, writerScript, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.keep-question-times-out',
  title: 'An unanswered Keep question goes back to the buttons and merges nothing',
  area: 'night',
  tags: ['night'],
  edge: true,
  timeout: 90000,
};

const ROW = 'css=.ck-morning-task';

export async function run(t) {
  t.fake.script(writerScript('night.txt', 'maybe later'));
  const { dir } = await startOneTaskNight(t);
  await t.waitFor('exact=Open Morning', { timeout: 60000 });
  await openMorning(t);

  await t.step('Press Keep once');
  await sclick(t, 'exact=Keep', { within: ROW });
  await t.expect.text(/Merge into \S+\?/);

  await t.step('Do nothing: the question goes away on its own');
  await t.expect.eventually(async () => !/Merge into \S+\?/.test(await t.page.text()), { timeout: 8000, message: 'the question to go away' });

  await t.step('Both buttons are back and the project is untouched');
  await t.expect.visible('exact=Keep', { within: ROW });
  await t.expect.visible('exact=Throw away', { within: ROW });
  t.expect.equal(readFile(dir, 'night.txt'), null);
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
