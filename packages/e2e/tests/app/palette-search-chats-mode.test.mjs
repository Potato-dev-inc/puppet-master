// Ctrl+Shift+F opens the palette straight in "Search all chats"; with an empty box Backspace goes back to the commands.
import { calm } from './_area.mjs';

export const meta = {
  id: 'app.palette-search-chats-mode',
  title: 'Ctrl+Shift+F opens Search all chats and Backspace on an empty box goes back',
  area: 'app',
  tags: ['palette', 'search', 'keyboard'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await t.step('Press Ctrl+Shift+F');
  await t.press('Ctrl+Shift+F');
  await t.waitFor('aria=Search all chats');
  await t.expect.visible('Search what was said');
  await t.expect.text('Chats you have opened');

  await t.step('An empty box tells what to do');
  await t.expect.text('Type to search every chat you have opened.');

  await t.step('Text that no chat holds finds nothing');
  await t.type('zzqqxx-not-said-anywhere');
  await t.expect.text('No matches');

  await t.step('Clear the box, then Backspace once more: back to the commands');
  await t.page.fill('aria=Search all chats', '');
  await t.press('Backspace');
  await t.waitFor('aria=Search commands');
  await t.expect.visible('Switch pane, start an agent, run an action');
  await t.expect.text('Open settings');

  await t.step('Esc closes the palette');
  await t.press('Escape');
  await t.expect.notVisible('aria=Search commands');
}
