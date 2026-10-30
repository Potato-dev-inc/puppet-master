// The sidebar pill opens a small menu (+15 min, End Focus); its own End button ends Focus at once.
import { focusMinutesLeft, focusReset, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'focus.pill-menu',
  title: 'The sidebar pill has a menu with +15 min and End Focus, and an End button',
  area: 'focus',
  tags: ['focus'],
};

export async function run(t) {
  try {
    await sclick(t, 'exact=25 min');
    await t.waitFor('css=[data-testid=focus-pill]');

    await t.step('Open the pill menu');
    await sclick(t, 'css=.ck-focus-main');
    await t.waitFor('css=.ck-focus-menu');
    await t.expect.visible({ exact: '+15 min', within: 'css=.ck-focus-menu' });
    await t.expect.visible({ exact: 'End Focus', within: 'css=.ck-focus-menu' });

    await t.step('+15 min in the menu adds time and closes the menu');
    await t.click({ exact: '+15 min', within: 'css=.ck-focus-menu' });
    await t.expect.eventually(async () => (await focusMinutesLeft(t)) >= 38, { message: 'about 40 minutes are left' });
    await t.expect.notVisible('css=.ck-focus-menu');

    await t.step('Escape closes the menu without changing anything');
    await sclick(t, 'css=.ck-focus-main');
    await t.waitFor('css=.ck-focus-menu');
    await t.press('Escape');
    await t.expect.notVisible('css=.ck-focus-menu');
    t.expect.ok((await focusMinutesLeft(t)) >= 38, 'Focus is still on');

    await t.step('The End button on the pill ends Focus at once');
    await sclick(t, 'css=.ck-focus-end');
    await t.waitFor('css=[data-focus=off]');
    await t.expect.notVisible('css=[data-testid=focus-pill]');
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;
