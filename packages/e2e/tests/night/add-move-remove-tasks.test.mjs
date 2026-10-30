// Several tasks in one night: add, reorder with the arrows, remove; the worst-case line follows the list.
import { clickCss, gitProject, openTonight, resetNight, writeTask, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.add-move-remove-tasks',
  title: 'Tasks can be added, moved up and down and removed, and the worst case follows',
  area: 'night',
  tags: ['night'],
};

const words = (t, n) => t.page.eval(`document.querySelector('textarea[aria-label="Task ${n}"]')?.value ?? null`);

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: gitProject(t, 'night-app') });
  await openTonight(t);
  await writeTask(t, 1, 'first thing');

  await t.step('Add a second and a third task');
  await clickCss(t, 'button.ck-night-add');
  await t.waitFor('aria=Task 2');
  await writeTask(t, 2, 'second thing');
  await clickCss(t, 'button.ck-night-add');
  await t.waitFor('aria=Task 3');
  await writeTask(t, 3, 'third thing');
  await t.expect.text('+ Add a task (3 of 10)');
  await t.expect.text('Worst case: 3 tasks x 12 turns = 36 turns, at most 2 at a time, until 07:00.');

  await t.step('The first task cannot move up, the last cannot move down');
  t.expect.equal(await t.page.eval(`document.querySelector('button[aria-label="Move task 1 up"]').disabled`), true);
  t.expect.equal(await t.page.eval(`document.querySelector('button[aria-label="Move task 3 down"]').disabled`), true);

  await t.step('Move the third task up: it swaps with the second');
  await clickCss(t, 'button[aria-label="Move task 3 up"]');
  await t.expect.eventually(async () => (await words(t, 2)) === 'third thing' && (await words(t, 3)) === 'second thing', { message: 'the two tasks swap places' });

  await t.step('Move the first task down');
  await clickCss(t, 'button[aria-label="Move task 1 down"]');
  await t.expect.eventually(async () => (await words(t, 1)) === 'third thing' && (await words(t, 2)) === 'first thing', { message: 'the first task moves down' });

  await t.step('Remove the middle task');
  await clickCss(t, 'button[aria-label="Remove task 2"]');
  await t.expect.eventually(async () => (await words(t, 3)) === null, { message: 'only two tasks are left' });
  t.expect.equal(await words(t, 1), 'third thing');
  t.expect.equal(await words(t, 2), 'second thing');
  await t.expect.text('+ Add a task (2 of 10)');
  await t.expect.text('Worst case: 2 tasks x 12 turns = 24 turns, at most 2 at a time, until 07:00.');
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
