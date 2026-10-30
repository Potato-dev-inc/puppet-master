// Only one night at a time: while one is running, planning another says to finish or stop tonight's run first.
import { blockedLine, isDisabled, openTonight, resetNight, slowScript, startOneTaskNight, tickPlan, writeTask, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.second-night-refused',
  title: 'A second night cannot start while one is running',
  area: 'night',
  tags: ['night'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  try {
    await startOneTaskNight(t, { text: 'the first night task', script: slowScript(120000), name: 'first-app' });
    await t.waitFor('css=[data-testid=night-chip]');
    await t.expect.text(/0 of 1 until \d\d:\d\d/);

    await t.step('Plan another night while the first one runs');
    await openTonight(t);
    await writeTask(t, 1, 'a second night task');
    await tickPlan(t, 'Nimbus');

    await t.step('Start is off and says to finish or stop tonight\'s run first');
    await t.expect.eventually(async () => (await blockedLine(t)) === 'Finish or stop tonight\'s run first.', { message: 'the busy sentence' });
    t.expect.equal(await isDisabled(t, 'Start Night Shift'), true);

    await t.step('Cancel closes the sheet and the first night goes on');
    await t.click('exact=Cancel');
    await t.expect.notVisible('exact=Start Night Shift');
    await t.expect.text(/0 of 1 until \d\d:\d\d/);
  } finally {
    await resetNight(t);
  }
}

export const setup = nightSetup;
