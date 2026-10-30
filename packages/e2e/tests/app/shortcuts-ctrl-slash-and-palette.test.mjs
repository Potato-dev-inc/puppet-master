// Ctrl+/ and the palette row "Keyboard shortcuts" open the same sheet.
import { openPalette, calm } from './_area.mjs';

export const meta = {
  id: 'app.shortcuts-ctrl-slash-and-palette',
  title: 'Ctrl+/ and the palette row also open the shortcuts sheet',
  area: 'app',
  tags: ['keyboard', 'palette'],
};

export async function run(t) {
  await calm(t);
  await t.step('Press Ctrl+/');
  await t.press('Ctrl+/');
  await t.waitFor('css=.ck-sheet--shortcuts');
  await t.press('Escape');
  await t.expect.notVisible('css=.ck-sheet--shortcuts');

  await t.step('Ctrl+K, type "keyboard", Enter');
  await openPalette(t);
  await t.type('keyboard');
  await t.press('Enter');
  await t.waitFor('css=.ck-sheet--shortcuts');
  await t.expect.notVisible('aria=Search commands');
  await t.expect.text('Next or previous card in the Inbox');
}
