// Edge case: below 1100 px of width the rail has no room. It is hidden and its button is gone; widening brings it back.
import { calm, openPane, resizeWindow } from './_area.mjs';

export const meta = {
  id: 'panes.rail-hidden-in-narrow-window',
  title: 'A narrow window hides the side rail and its button, a wide one brings them back',
  area: 'panes',
  tags: ['rail', 'resize'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await t.waitFor('css=.ck-rail');

  try {
    await t.step('Make the window narrow');
    await resizeWindow(t, 980, 700);
    await t.expect.notVisible('css=.ck-rail');
    await t.expect.notVisible('aria=Hide session details');
    await t.expect.notVisible('aria=Show session details');
    await t.expect.text('NIMBUS 0.4.2');

    await t.step('Make it wide again');
    await resizeWindow(t, 1280, 800);
    await t.waitFor('css=.ck-rail');
    await t.expect.visible('aria=Hide session details');
  } finally {
    await resizeWindow(t, 1280, 800).catch(() => {});
  }
}
