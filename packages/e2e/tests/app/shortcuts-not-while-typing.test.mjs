// Edge case: "?" is just a character in a text box and in a terminal. It must not pop the sheet there.
import { openPalette, focusTerminal, calm } from './_area.mjs';

export const meta = {
  id: 'app.shortcuts-not-while-typing',
  title: 'A ? typed in a text box or in a terminal does not open the sheet',
  area: 'app',
  tags: ['keyboard'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  const pane = await t.spawnPane('nimbus');

  await t.step('Type a question mark into the palette box');
  await openPalette(t);
  await t.press('?');
  await t.expect.value('aria=Search commands', '?');
  await t.expect.notVisible('css=.ck-sheet--shortcuts');
  await t.press('Escape');

  await t.step('Open the pane and type a question mark into its terminal');
  await t.click('exact=Nimbus', { within: 'css=.ck-side' });
  await t.expect.text('NIMBUS 0.4.2');
  await focusTerminal(t);
  await t.press('?');
  await t.expect.notVisible('css=.ck-sheet--shortcuts');
  await t.expect.eventually(async () => (await pane.screen()).includes('> ?'), { message: 'the terminal got the ? as typed text' });
}
