// The Tonight sheet opens with one task, the plans, a stop time and the checks, and says why Start is off.
import { blockedLine, gitProject, isDisabled, openTonight, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.sheet-defaults',
  title: 'The Tonight sheet opens with one task, the plans and Start switched off',
  area: 'night',
  tags: ['night', 'smoke'],
};

export async function run(t) {
  const dir = gitProject(t, 'night-app');
  await t.spawnPane('nimbus', { cwd: dir });

  await t.step('Open Tonight from the plus menu');
  await openTonight(t);

  await t.step('One empty task, on the project of the open pane');
  await t.expect.text('Puppet Master works through these while you sleep.');
  await t.expect.text('+ Add a task (1 of 10)');
  await t.expect.value('aria=Task 1', '');
  await t.expect.text('night-app');

  await t.step('The plans to use, with Nimbus ready');
  await t.expect.text('Plans to use');
  await t.expect.text('(one login per company)');
  await t.expect.visible({ text: 'Nimbus', within: 'css=.ck-night-plans' });

  await t.step('The stop time, how many at once and keep awake');
  await t.expect.value('aria=Stop at', '07:00');
  await t.expect.text('Keep this computer awake until it is done');
  await t.expect.text('Worst case: 1 task x 12 turns = 12 turns, at most 1 at a time, until 07:00.');

  await t.step('The checks before you go');
  await t.expect.text('Undo is on');
  await t.expect.text('Every project is a git project with a commit');
  await t.expect.text('1 task has no tests');

  await t.step('Start is off and says why in one line');
  t.expect.equal(await isDisabled(t, 'Start Night Shift'), true, 'Start is disabled');
  await t.expect.eventually(async () => (await blockedLine(t)) === 'Tick at least one plan to use.', { message: 'the reason is the plans' });
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
