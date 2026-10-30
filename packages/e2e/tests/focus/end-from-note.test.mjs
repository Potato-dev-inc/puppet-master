// End Focus in the Inbox note stops it at once: the chips return and the pill goes.
import { focusReset, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'focus.end-from-note',
  title: 'End Focus in the Inbox note stops it and brings the chips back',
  area: 'focus',
  tags: ['focus'],
};

export async function run(t) {
  try {
    await sclick(t, 'exact=90 min');
    await t.waitFor('css=[data-focus=on]');
    await t.expect.visible('css=[data-testid=focus-pill]');

    await t.step('Press End Focus');
    await sclick(t, 'exact=End Focus');

    await t.step('The chips are back and the sidebar pill is gone');
    await t.waitFor('css=[data-focus=off]');
    for (const m of [25, 45, 90]) await t.expect.visible(`exact=${m} min`);
    await t.expect.notVisible('css=[data-testid=focus-pill]');

    await t.step('Nothing is left over: no summary card for an empty session');
    await t.expect.noText('While you focused');
    const view = (await t.bridge.get('/focus')).body;
    t.expect.equal(view.on, false);
    t.expect.equal(view.endsAtMs, null);
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;
