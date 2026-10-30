// Words are not enough: at least one plan must be ticked. Unticking the plan turns Start off again.
import { blockedLine, gitProject, isDisabled, openTonight, tickPlan, writeTask, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.start-needs-a-plan',
  title: 'Start stays off until a plan is ticked, and off again when it is unticked',
  area: 'night',
  tags: ['night'],
  edge: true,
};

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: gitProject(t, 'night-app') });
  await openTonight(t);
  await writeTask(t, 1, 'add a changelog line');

  await t.step('Words but no plan: Start is off');
  await t.expect.eventually(async () => (await blockedLine(t)) === 'Tick at least one plan to use.', { message: 'asks for a plan' });
  t.expect.equal(await isDisabled(t, 'Start Night Shift'), true);

  await t.step('Tick Nimbus: Start turns on');
  await tickPlan(t, 'Nimbus');
  await t.expect.eventually(async () => (await isDisabled(t, 'Start Night Shift')) === false, { message: 'Start is enabled' });

  await t.step('Untick it: Start is off again');
  await tickPlan(t, 'Nimbus');
  await t.expect.eventually(async () => (await isDisabled(t, 'Start Night Shift')) === true, { message: 'Start is disabled again' });
  await t.expect.text('Tick at least one plan to use.');
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
