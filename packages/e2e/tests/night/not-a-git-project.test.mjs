// Night Shift works in a separate copy, so a folder without Git cannot be used: the sheet says so and keeps Start off.
import { blockedLine, isDisabled, openTonight, pickProject, resetNight, tickPlan, writeTask, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.not-a-git-project',
  title: 'A folder that is not a git project is marked and keeps Start off',
  area: 'night',
  tags: ['night'],
  edge: true,
};

export async function run(t) {
  const plain = t.project('plain-folder'); // no git here
  await t.spawnPane('nimbus', { cwd: plain });
  await openTonight(t);
  await pickProject(t, 1, 'plain-folder');

  await t.step('The folder is listed as not a git project');
  await t.expect.text('plain-folder (not a git project)');
  await t.expect.text('A project is not a git project with a commit');

  await t.step('Even with words and a plan, Start explains the folder');
  await writeTask(t, 1, 'try to do something');
  await tickPlan(t, 'Nimbus');
  await t.expect.eventually(async () => (await blockedLine(t)).startsWith('Task 1: this project is not a git project with a commit.'), { message: 'the not-git reason' });
  t.expect.equal(await isDisabled(t, 'Start Night Shift'), true);
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
