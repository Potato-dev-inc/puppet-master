// The Focus state the bridge (and so the phone) reads has times and counts only, never text of what was held.
import { focusReset, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'focus.bridge-state-counts-only',
  title: 'The Focus state the phone reads holds times and counts only',
  area: 'focus',
  tags: ['focus'],
  edge: true,
};

export async function run(t) {
  try {
    await t.step('Off: the state says so');
    let view = (await t.bridge.get('/focus')).body;
    t.expect.equal(view.on, false);
    t.expect.equal(view.endsAtMs, null);
    t.expect.equal(view.minutes, 45);

    await t.step('Start Focus on the screen');
    await sclick(t, 'exact=25 min');
    await t.waitFor('css=[data-focus=on]');

    await t.step('The state now has an end time in the future and counts');
    view = (await t.bridge.get('/focus')).body;
    t.expect.equal(view.on, true);
    t.expect.ok(view.endsAtMs > Date.now(), 'the end is in the future');
    t.expect.ok(view.endsAtMs - Date.now() <= 25 * 60_000 + 5000, 'the end is about 25 minutes away');
    t.expect.equal(view.heldTotal, 0);
    t.expect.equal(view.waiting, 0);

    await t.step('Only numbers, booleans and nothing else: no text of any held thing');
    const kinds = Object.fromEntries(Object.entries(view).map(([k, v]) => [k, v === null ? 'null' : typeof v]));
    for (const [key, kind] of Object.entries(kinds)) {
      t.expect.ok(['number', 'boolean', 'null'].includes(kind), `${key} is a ${kind}, not text`);
    }
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;
