// A night must run at least 30 minutes and at most 14 hours; Start says so when the stop time breaks that.
import { blockedLine, clockIn, gitProject, isDisabled, openTonight, resetNight, setTime, tickPlan, writeTask, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.stop-time-too-soon',
  title: 'A stop time under 30 minutes away is refused, and one over 14 hours as well',
  area: 'night',
  tags: ['night'],
  edge: true,
};

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: gitProject(t, 'night-app') });
  await openTonight(t);
  await writeTask(t, 1, 'a small task');
  await tickPlan(t, 'Nimbus');

  await t.step('Stop in 10 minutes: too soon');
  const soon = new Date(Date.now() + 10 * 60_000);
  await setTime(t, 'Stop at', `${String(soon.getHours()).padStart(2, '0')}:${String(soon.getMinutes()).padStart(2, '0')}`);
  await t.expect.eventually(async () => (await blockedLine(t)) === 'Pick a stop time at least 30 minutes after the start.', { message: 'the too-soon reason' });
  t.expect.equal(await isDisabled(t, 'Start Night Shift'), true);

  await t.step('Stop 16 hours from now: too late');
  await setTime(t, 'Stop at', clockIn(16));
  await t.expect.eventually(async () => (await blockedLine(t)) === 'Pick a stop time within 14 hours of the start.', { message: 'the too-late reason' });
  t.expect.equal(await isDisabled(t, 'Start Night Shift'), true);

  await t.step('Stop in 5 hours: fine');
  await setTime(t, 'Stop at', clockIn(5));
  await t.expect.eventually(async () => (await isDisabled(t, 'Start Night Shift')) === false, { message: 'Start is enabled' });
  t.expect.equal(await blockedLine(t), '');
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
