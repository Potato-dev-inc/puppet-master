// Ctrl+K opens the command palette, Esc and a click outside close it, and Ctrl+K again toggles it.
import { openPalette, calm } from './_area.mjs';

export const meta = {
  id: 'app.palette-opens-and-closes',
  title: 'Ctrl+K opens the command palette and Esc or a click outside closes it',
  area: 'app',
  tags: ['smoke', 'palette', 'keyboard'],
};

export async function run(t) {
  await calm(t);
  await t.step('Press Ctrl+K: the palette opens with its list');
  await openPalette(t);
  await t.expect.visible('Switch pane, start an agent, run an action');
  await t.expect.text('Keyboard shortcuts');
  await t.expect.text('Open settings');

  await t.step('Esc closes it');
  await t.press('Escape');
  await t.expect.notVisible('aria=Search commands');

  await t.step('Open it again and click outside the box');
  await openPalette(t);
  await t.click([20, 400]);
  await t.expect.notVisible('aria=Search commands');

  await t.step('Ctrl+K a second time closes it again (toggle)');
  await openPalette(t);
  await t.press('Ctrl+K');
  await t.expect.notVisible('aria=Search commands');
}
