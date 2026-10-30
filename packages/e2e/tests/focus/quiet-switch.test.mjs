// "Make cards that wait for you quiet during Focus" is on by default and can be turned off and on again.
import { flip, focusReset, isOn, openSettings, scrollToCss, switchOf } from '../night/_area.mjs';

export const meta = {
  id: 'focus.quiet-switch',
  title: 'The quiet-cards switch is on by default and can be flipped',
  area: 'focus',
  tags: ['focus'],
};

const LABEL = 'Make cards that wait for you quiet during Focus';

export async function run(t) {
  try {
    await t.step('Open Settings > General > Focus Time');
    await openSettings(t, 'General');
    await scrollToCss(t, `button[role=switch][aria-label="${LABEL}"]`);
    await t.expect.text('Urgent things always come through');
    await t.expect.text('Focus never answers anything for you.');
    t.expect.equal(await isOn(t, LABEL), true, 'quiet cards is on by default');

    await t.step('Turn it off');
    await flip(t, LABEL);
    await t.expect.eventually(async () => !(await isOn(t, LABEL)), { message: 'the switch turns off' });
    const saved = await t.bridge.get('/focus');
    t.expect.equal(saved.status, 200);

    await t.step('Turn it on again');
    await flip(t, LABEL);
    await t.expect.eventually(() => isOn(t, LABEL), { message: 'the switch turns on' });
    await t.expect.visible(switchOf(LABEL));
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;
