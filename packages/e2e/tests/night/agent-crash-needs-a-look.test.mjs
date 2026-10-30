// An agent that crashes in the night leaves its task under "Needs a look" with the reason and the way on.
import { openMorning, startOneTaskNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.agent-crash-needs-a-look',
  title: 'A crashing agent leaves its task under Needs a look with a plain reason',
  area: 'night',
  tags: ['night'],
  edge: true,
  timeout: 90000,
};

const CRASHES = `
boot_ms = 100
inline = true
[[turn]]
steps = [ { do = "think", ms = 300 }, { do = "crash", message = "boom", code = 101 } ]
`;

export async function run(t) {
  await startOneTaskNight(t, { text: 'do something that crashes', script: CRASHES });
  await t.waitFor('exact=Open Morning', { timeout: 60000 });

  await t.step('The chip counts the task that needs you');
  await t.expect.text('0 of 1 done');
  await t.expect.visible('css=[title="1 need you"]');

  await t.step('The Morning page explains the crash and what to do');
  await openMorning(t);
  await t.expect.text('0 of 1 done. 1 needs you.');
  await t.expect.text('NEEDS A LOOK (1)');
  await t.expect.text('the agent exited while it was working on this turn');
  await t.expect.text('Run the task again, or open its terminal to see why.');

  await t.step('Nothing was merged and nothing counts as ready');
  await t.expect.noText('READY TO KEEP');
  await t.expect.visible({ exact: 'Throw away', within: 'css=.ck-morning-task' });
}

export const setup = nightSetup;
