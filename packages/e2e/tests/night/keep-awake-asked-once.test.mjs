// "Keep this computer awake" is asked once: the first switch-on explains it; after any answer the question is not asked again.
import { gitProject, openTonight, resetNight, sheetSwitchOn, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.keep-awake-asked-once',
  title: 'The keep-awake switch explains itself once, then switches on without asking',
  area: 'night',
  tags: ['night'],
  edge: true,
};

const SWITCH = 'aria=Keep this computer awake until it is done';
const WORDS = 'Puppet Master will stop this computer from sleeping until Night Shift ends';

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: gitProject(t, 'night-app') });
  await openTonight(t);
  t.expect.equal(await sheetSwitchOn(t, 'Keep this computer awake until it is done'), false, 'starts off');

  await t.step('Switch it on: the sheet explains what it does');
  await t.click(SWITCH);
  await t.expect.text(WORDS);
  await t.expect.visible('exact=Keep it awake');

  await t.step('Say no: the switch stays off');
  await t.click('exact=Don\'t');
  await t.expect.noText(WORDS);
  t.expect.equal(await sheetSwitchOn(t, 'Keep this computer awake until it is done'), false);

  await t.step('Switch it on again: no second question, it just turns on');
  await t.click(SWITCH);
  await t.expect.eventually(() => sheetSwitchOn(t, 'Keep this computer awake until it is done'), { message: 'the switch turns on' });
  await t.expect.noText(WORDS);

  await t.step('Switch it off again');
  await t.click(SWITCH);
  await t.expect.eventually(async () => !(await sheetSwitchOn(t, 'Keep this computer awake until it is done')), { message: 'the switch turns off' });
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
