// A task with no words cannot start; the sheet says which task and what is missing.
import { blockedLine, gitProject, isDisabled, openTonight, tickPlan, writeTask, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.start-needs-words',
  title: 'A task without words keeps Start off until you write them',
  area: 'night',
  tags: ['night'],
  edge: true,
};

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: gitProject(t, 'night-app') });
  await openTonight(t);

  await t.step('Tick the Nimbus plan but leave the task empty');
  await tickPlan(t, 'Nimbus');
  await t.expect.eventually(async () => (await blockedLine(t)) === 'Task 1 needs words: what should it do?', { message: 'Start says the task needs words' });
  t.expect.equal(await isDisabled(t, 'Start Night Shift'), true);

  await t.step('Spaces only do not count as words');
  await writeTask(t, 1, '     ');
  await t.expect.eventually(async () => (await blockedLine(t)).startsWith('Task 1 needs words'), { message: 'blank text is still empty' });

  await t.step('Write the words: Start turns on and the line goes away');
  await writeTask(t, 1, 'tidy the readme');
  await t.expect.eventually(async () => (await isDisabled(t, 'Start Night Shift')) === false, { message: 'Start is enabled' });
  t.expect.equal(await blockedLine(t), '');
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
