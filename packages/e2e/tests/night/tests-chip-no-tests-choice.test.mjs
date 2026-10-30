// The Tests chip: "Pick tests…" until something is chosen; a typed command becomes the chip; "No tests for this project" is a
// choice too and the sheet then stops warning about it.
import { gitProject, nightSetup, openTonight, sclick } from './_area.mjs';

export const meta = {
  id: 'night.tests-chip-no-tests-choice',
  title: 'The Tests chip takes a typed command or an explicit No tests choice',
  area: 'night',
  tags: ['night'],
};

const chip = (t) => t.page.eval(`document.querySelector('button[aria-label="Tests for task 1"]').innerText`);

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: gitProject(t, 'tests-app') });
  await openTonight(t);

  await t.step('Nothing is declared: the chip asks and the sheet warns');
  t.expect.equal(await chip(t), 'Pick tests…');
  await t.expect.text('No tests declared. It will run, but nothing will be checked.');
  await t.expect.text('1 task has no tests');

  await t.step('Say there are no tests for this project');
  await sclick(t, 'aria=Tests for task 1');
  await t.waitFor('exact=No tests for this project');
  await t.click('exact=No tests for this project');
  await t.expect.eventually(async () => (await chip(t)) === 'No tests', { message: 'the chip says No tests' });

  await t.step('The note under the task still says nothing will be checked');
  await t.expect.text('No tests declared. It will run, but nothing will be checked.');

  await t.step('Now type a command: it becomes the chip and the warning goes');
  await sclick(t, 'aria=Tests for task 1');
  await t.fill('aria=Add a test command', 'npm test');
  await t.expect.visible('exact=Add');
  await t.press('Enter');
  await t.expect.eventually(async () => (await chip(t)) === 'npm test', { message: 'the chip shows the command' });
  await t.page.eval(`document.querySelector('button[aria-label="Tests for task 1"]').click()`);
  await t.expect.text('Every task has tests');
  await t.expect.noText('1 task has no tests');
}

export const setup = nightSetup;
