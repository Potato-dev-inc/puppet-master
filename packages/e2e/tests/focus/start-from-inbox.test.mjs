// Focus starts from the chips at the top of the Inbox; the note and the sidebar pill show the time left.
import { focusMinutesLeft, focusReset, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'focus.start-from-inbox',
  title: 'The 45 min chip starts Focus and the note and sidebar pill count down',
  area: 'focus',
  tags: ['focus', 'smoke'],
};

export async function run(t) {
  try {
    await t.step('The Inbox offers three lengths');
    await t.expect.visible({ exact: 'Focus', within: 'css=[data-focus=off]' });
    for (const m of [25, 45, 90]) await t.expect.visible(`exact=${m} min`);
    await t.expect.noText('End Focus');

    await t.step('Press 45 min');
    await sclick(t, 'exact=45 min');

    await t.step('The note shows the time left and what is held');
    await t.waitFor('css=[data-focus=on]');
    const left = await focusMinutesLeft(t);
    t.expect.ok(left >= 43 && left <= 45, `about 45 minutes are left (${left})`);
    await t.expect.text('Nothing held yet. Things that need you still show.');
    await t.expect.visible('exact=+15 min');
    await t.expect.visible('exact=End Focus');

    await t.step('The sidebar has a Focus pill with the same time and an End button');
    await t.expect.visible('css=[data-testid=focus-pill]');
    await t.expect.text(/Focus · \d+ min left/);
    await t.expect.visible('aria=End Focus');

    await t.step('The length chips are gone while Focus is on');
    await t.expect.notVisible('exact=25 min');
    await t.expect.notVisible('exact=90 min');
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;
