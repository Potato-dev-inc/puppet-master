// "Ask to move it back when the first agent resets" is on by default; the choice is saved.
import { flip, isOn } from '../night/_area.mjs';
import { keepGoingReset, openKeepGoing } from './_area.mjs';

export const meta = {
  id: 'keepgoing.ask-back-switch',
  title: 'The ask-to-move-back switch is on by default and keeps its choice',
  area: 'keepgoing',
  tags: ['keepgoing'],
};

const LABEL = 'Ask to move it back when the first agent resets';

export async function run(t) {
  try {
    await openKeepGoing(t);
    await t.expect.text('Asks once, after the reset time. Never moves a task back on its own.');
    t.expect.equal(await isOn(t, LABEL), true, 'on by default');

    await t.step('Turn it off');
    await flip(t, LABEL);
    await t.expect.eventually(async () => !(await isOn(t, LABEL)), { message: 'the switch turns off' });

    await t.step('Leave Settings and come back: it is still off');
    await t.click('exact=Inbox');
    await openKeepGoing(t);
    t.expect.equal(await isOn(t, LABEL), false);

    await t.step('Turn it on again');
    await flip(t, LABEL);
    await t.expect.eventually(() => isOn(t, LABEL), { message: 'the switch turns on' });
  } finally {
    await keepGoingReset(t);
  }
}

export const setup = keepGoingReset;
