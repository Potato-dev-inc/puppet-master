// "Start at" a later time: the chip says when it starts, no agent runs yet, and Stop cancels the plan.
import { clockIn, gitProject, nightSetup, openTonight, resetNight, setTime, sclick, tickPlan, writeTask } from './_area.mjs';

export const meta = {
  id: 'night.scheduled-start',
  title: 'A night set for later waits, shows when it starts and can be stopped',
  area: 'night',
  tags: ['night'],
  timeout: 90000,
};

export async function run(t) {
  try {
    await t.spawnPane('nimbus', { cwd: gitProject(t, 'night-app') });
    await openTonight(t);
    await writeTask(t, 1, 'a task for later');
    await tickPlan(t, 'Nimbus');

    await t.step('Choose a start time one hour from now and a stop time five hours from now');
    const startAt = clockIn(1);
    await sclick(t, 'exact=At');
    await setTime(t, 'Start at', startAt);
    await setTime(t, 'Stop at', clockIn(5));
    await t.expect.eventually(async () => (await t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === 'Start Night Shift').disabled`)) === false, { message: 'Start is enabled' });
    await t.click('exact=Start Night Shift');

    await t.step('The chip says when it starts');
    await t.waitFor('css=[data-testid=night-chip]');
    await t.expect.text(`Starts ${startAt}`);
    await t.expect.visible('exact=Stop');

    await t.step('Nothing runs yet: no task pane was opened');
    t.expect.equal((await t.panes()).length, 1, 'only the pane the test started');

    await t.step('Stop cancels the plan');
    await t.click('exact=Stop');
    await t.expect.text('Stops everything now. Copies are kept.');
    await t.click('exact=Stop Night Shift');
    await t.waitFor('exact=Open Morning');
    t.expect.equal((await t.panes()).length, 1, 'still no task pane');
  } finally {
    await resetNight(t);
  }
}

export const setup = nightSetup;
