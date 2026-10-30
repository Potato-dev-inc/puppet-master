// The command palette starts and manages Focus too: three lengths while off; End Focus, Add 15 minutes and the lengths while on.
import { focusMinutesLeft, focusReset } from '../night/_area.mjs';

export const meta = {
  id: 'focus.palette-rows',
  title: 'The command palette starts Focus, extends it and ends it',
  area: 'focus',
  tags: ['focus'],
};

const palette = async (t, word) => {
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
  await t.type(word);
};

export async function run(t) {
  try {
    await t.step('While Focus is off the palette offers three lengths and no End');
    await palette(t, 'focus');
    for (const m of [25, 45, 90]) await t.expect.visible(`exact=Focus ${m} minutes`);
    await t.expect.notVisible({ exact: 'End Focus', within: 'css=[aria-label="Command palette"]' });

    await t.step('Pick Focus 25 minutes');
    await t.click('exact=Focus 25 minutes');
    await t.waitFor('css=[data-focus=on]');
    t.expect.ok((await focusMinutesLeft(t)) <= 25, 'about 25 minutes');

    await t.step('While it is on the palette offers End Focus and Add 15 minutes');
    await palette(t, 'focus');
    await t.expect.visible({ exact: 'End Focus', within: 'css=[aria-label="Command palette"]' });
    await t.expect.visible('exact=Add 15 minutes to Focus');
    await t.click('exact=Add 15 minutes to Focus');
    await t.expect.eventually(async () => (await focusMinutesLeft(t)) >= 38, { message: 'about 40 minutes are left' });

    await t.step('Starting a length again starts over from now');
    await palette(t, 'focus');
    await t.expect.text('Starts again from now');
    await t.click('exact=Focus 90 minutes');
    await t.expect.eventually(async () => (await focusMinutesLeft(t)) >= 88, { message: 'about 90 minutes are left' });

    await t.step('End Focus from the palette');
    await palette(t, 'focus');
    await t.click({ exact: 'End Focus', within: 'css=[aria-label="Command palette"]' });
    await t.waitFor('css=[data-focus=off]');
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;
