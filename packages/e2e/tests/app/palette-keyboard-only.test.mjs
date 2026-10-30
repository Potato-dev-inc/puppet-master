// Keyboard only: Ctrl+K, type, arrow down and Enter run a palette action (here: open the Toolbelt page in Settings).
import { openPalette, calm } from './_area.mjs';

export const meta = {
  id: 'app.palette-keyboard-only',
  title: 'The palette works with the keyboard alone: type, arrows, Enter',
  area: 'app',
  tags: ['palette', 'keyboard'],
};

export async function run(t) {
  await calm(t);
  await t.step('Ctrl+K and type "mcp"');
  await openPalette(t);
  await t.type('mcp');
  await t.expect.text('Toolbelt');

  await t.step('The first row is highlighted: Enter runs it');
  await t.expect.eventually(async () => (await t.page.eval(`document.querySelector('.ck-palette-row.is-active .ck-palette-title')?.innerText`)) === 'Toolbelt…', { message: 'Toolbelt… is highlighted' });
  await t.press('Enter');

  await t.step('The palette closed and Settings opened on Bridge & MCP');
  await t.expect.notVisible('aria=Search commands');
  await t.expect.text('How other tools and agents connect to Puppet Master.');
  await t.expect.text('Where other tools reach Puppet Master on this computer.');

  await t.step('Esc leaves Settings (keyboard only)');
  await t.press('Escape');
  await t.expect.noText('How other tools and agents connect to Puppet Master.');
  await t.expect.text('Nothing needs you');
}
