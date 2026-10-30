// "More" opens the turns and minutes of one task; values outside 3 to 40 turns or 20 to 240 minutes keep Start off.
import { blockedLine, gitProject, isDisabled, openTonight, resetNight, tickPlan, writeTask, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.budget-out-of-range',
  title: 'Turns or minutes outside the allowed range keep Start off with a plain reason',
  area: 'night',
  tags: ['night'],
  edge: true,
};

const setNumber = (t, label, value) =>
  t.page.eval(`(() => {
    const row = [...document.querySelectorAll('label.ck-night-inline')].find((l) => l.innerText.trim().startsWith(${JSON.stringify(label)}));
    const el = row.querySelector('input');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(String(value))});
    el.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: gitProject(t, 'night-app') });
  await openTonight(t);
  await writeTask(t, 1, 'a small task');
  await tickPlan(t, 'Nimbus');
  await t.expect.eventually(async () => (await isDisabled(t, 'Start Night Shift')) === false, { message: 'Start is on with the defaults' });

  await t.step('Open More: the defaults are 12 turns and 90 minutes');
  await t.click('More ▸');
  await t.expect.eventually(async () => (await t.page.eval(`[...document.querySelectorAll('.ck-night-more input[type=number]')].map((i) => i.value).join(',')`)) === '12,90', { message: 'the defaults 12 and 90' });
  await t.expect.text('Agents may change the tests');

  await t.step('Two turns is too few');
  await setNumber(t, 'Turns', 2);
  await t.expect.eventually(async () => (await blockedLine(t)) === 'Task 1: turns must be 3 to 40, and minutes 20 to 240.', { message: 'the budget reason' });
  t.expect.equal(await isDisabled(t, 'Start Night Shift'), true);

  await t.step('Forty-one turns is too many');
  await setNumber(t, 'Turns', 41);
  await t.expect.eventually(async () => (await blockedLine(t)).startsWith('Task 1: turns must be 3 to 40'), { message: 'still refused' });

  await t.step('Ten minutes is too short, even with fine turns');
  await setNumber(t, 'Turns', 12);
  await setNumber(t, 'Minutes', 10);
  await t.expect.eventually(async () => (await blockedLine(t)).includes('minutes 20 to 240'), { message: 'minutes refused' });

  await t.step('Back inside the range: Start is on again');
  await setNumber(t, 'Minutes', 30);
  await t.expect.eventually(async () => (await isDisabled(t, 'Start Night Shift')) === false, { message: 'Start is enabled' });
  await t.expect.text('Worst case: 1 task x 12 turns = 12 turns');
}

// Labs switch on, and clear what an earlier night left (the tests of one run share the app data folder).
export const setup = nightSetup;
