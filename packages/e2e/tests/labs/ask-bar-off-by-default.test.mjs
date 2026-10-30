// Labs > Ask bar: off by default with its shortcut and agent choices listed but unusable. The test never turns it on, because
// the switch registers a keyboard shortcut for the whole of Windows.
import { isOn, labsOff, openSettings, scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'labs.ask-bar-off-by-default',
  title: 'The Ask bar is off with its shortcut and agent choices shown but not usable',
  area: 'labs',
  tags: ['labs'],
};

export async function run(t) {
  await openSettings(t, 'General');
  await t.waitFor('css=button[role=switch][aria-label="Ask bar"]');
  await scrollToCss(t, 'button[role=switch][aria-label="Ask bar"]');

  await t.step('The row explains the bar and says it is off');
  await t.expect.text('Open a small bar from anywhere in Windows to ask an agent a quick question. Off by default.');
  t.expect.equal(await isOn(t, 'Ask bar'), false);

  await t.step('The shortcut and agent lists are there but disabled while it is off');
  await t.expect.text('Ask bar shortcut');
  await t.expect.text('Ctrl+Alt+Space');
  await t.expect.text('Ask bar agent');
  const disabled = await t.page.eval(`[...document.querySelectorAll('select')].filter((s) => [...s.options].some((o) => o.text === 'Ctrl+Alt+Space' || o.text === 'Last used')).map((s) => s.disabled)`);
  t.expect.ok(disabled.length >= 2 && disabled.every(Boolean), `the two lists are disabled (${JSON.stringify(disabled)})`);

  await t.step('Attaching the clipboard is off by default too');
  t.expect.equal(await isOn(t, 'Attach clipboard by default'), false);
}

export const setup = labsOff;
