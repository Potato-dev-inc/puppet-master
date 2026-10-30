// A night holds at most 10 tasks: the Add button turns off at ten.
import { clickCss, gitProject, openTonight, resetNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.ten-task-limit',
  title: 'The sheet stops adding tasks at ten',
  area: 'night',
  tags: ['night'],
  edge: true,
};

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: gitProject(t, 'night-app') });
  await openTonight(t);

  await t.step('Add tasks until there are ten');
  for (let n = 1; n < 10; n++) await clickCss(t, 'button.ck-night-add');

  await t.step('The button now says ten of ten and is off');
  await t.expect.text('+ Add a task (10 of 10)');
  await t.expect.eventually(() => t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.includes('Add a task'))?.disabled === true`), { message: 'the Add button is disabled' });
  await t.expect.count('css=textarea[aria-label^="Task "]', 10);
  await t.expect.text('Worst case: 10 tasks x 12 turns = 120 turns, at most 2 at a time, until 07:00.');

  await t.step('Removing one gives the button back');
  await clickCss(t, 'button[aria-label="Remove task 10"]');
  await t.expect.text('+ Add a task (9 of 10)');
  await t.expect.eventually(() => t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.includes('Add a task'))?.disabled === false`), { message: 'the Add button is enabled again' });
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
