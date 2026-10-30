// "+15 min" adds fifteen minutes each time it is pressed.
import { focusMinutesLeft, focusReset, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'focus.extend-adds-15',
  title: '+15 min adds fifteen minutes to a running Focus, again and again',
  area: 'focus',
  tags: ['focus'],
};

export async function run(t) {
  try {
    await sclick(t, 'exact=25 min');
    await t.waitFor('css=[data-focus=on]');
    const start = await focusMinutesLeft(t);
    t.expect.ok(start >= 23 && start <= 25, `about 25 minutes (${start})`);

    await t.step('Press +15 min');
    await sclick(t, 'exact=+15 min');
    await t.expect.eventually(async () => (await focusMinutesLeft(t)) >= 38, { message: 'about 40 minutes are left' });
    const first = await focusMinutesLeft(t);
    t.expect.ok(first >= 38 && first <= 40, `about 40 minutes (${first})`);

    await t.step('Press it again');
    await sclick(t, 'exact=+15 min');
    await t.expect.eventually(async () => (await focusMinutesLeft(t)) >= 53, { message: 'about 55 minutes are left' });
    const second = await focusMinutesLeft(t);
    t.expect.ok(second >= 53 && second <= 55, `about 55 minutes (${second})`);
    await t.expect.text(`Focus · ${second} min left`);
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;
