// With a test declared (a command in the project), the night runs it in the copy after the turn and the Morning page
// says "tests green" with the counts. Without a declared test it only says that nothing was checked.
import { clockIn, gitProject, nightSetup, openMorning, openTonight, sclick, setTime, tickPlan, writeTask, writerScript } from './_area.mjs';

export const meta = {
  id: 'night.tests-green-when-declared',
  title: 'A declared test that passes makes the task ready with green tests',
  area: 'night',
  tags: ['night', 'smoke'],
  timeout: 120000,
};

const CHECK = "const fs = require('fs'); process.exit(fs.existsSync('night.txt') ? 0 : 1);\n";

export async function run(t) {
  const dir = gitProject(t, 'tests-app', { 'check.js': CHECK });
  await t.spawnPane('nimbus', { cwd: dir });
  await openTonight(t);
  await writeTask(t, 1, 'create the night file so the check passes');
  await tickPlan(t, 'Nimbus');

  await t.step('Declare the project\'s test: the command that proves the work');
  await sclick(t, 'aria=Tests for task 1');
  await t.fill('aria=Add a test command', 'node check.js');
  await t.press('Enter');
  await t.expect.eventually(async () => (await t.page.eval(`document.querySelector('button[aria-label="Tests for task 1"]').innerText`)) === 'node check.js', { message: 'the Tests chip shows the command' });
  await t.page.eval(`document.querySelector('button[aria-label="Tests for task 1"]').click()`); // close the list
  await t.expect.text('Every task has tests');

  await t.step('Start the night with an agent that writes the file');
  t.fake.script(writerScript('night.txt', 'x'));
  await setTime(t, 'Stop at', clockIn(4));
  await t.click('exact=Start Night Shift');
  await t.waitFor('exact=Open Morning', { timeout: 90000 });

  await t.step('The Morning page says the tests are green');
  await openMorning(t);
  await t.expect.text('1 of 1 done, tests green');
  await t.expect.text('READY TO KEEP (1)');
  await t.expect.text('Tests pass 1 of 1 checks');
  await t.expect.noText('nothing was checked');
}

export const setup = nightSetup;
