// Three ways to change the mode agree with each other: the sidebar switch, the palette row and the button in the main area.
import { openPalette, paletteTitles, calm, restoreOrchestration } from './_area.mjs';

export const meta = {
  id: 'app.mode-switch-controls',
  title: 'The sidebar switch, the palette and the main-area button all change the mode',
  area: 'app',
  tags: ['mode', 'palette'],
};

async function body(t) {
  await t.step('The palette offers Individual mode while in Orchestration');
  await openPalette(t);
  await t.type('individual mode');
  t.expect.includes(await paletteTitles(t), 'Switch to Individual mode');
  await t.expect.text('Every pane in its own window');

  await t.step('Enter: Individual mode');
  await t.press('Enter');
  await t.expect.text('Panes are in their own windows.');

  await t.step('The palette now offers Orchestration');
  await openPalette(t);
  await t.type('orchestration mode');
  t.expect.includes(await paletteTitles(t), 'Switch to Orchestration mode');
  await t.press('Escape');

  await t.step('The button in the main area switches back');
  await t.click('exact=Switch to Orchestration');
  await t.expect.noText('Panes are in their own windows.');
  await t.expect.text('Nothing needs you');

  await t.step('The sidebar switch goes to Individual and back');
  await t.click('exact=Individual');
  await t.expect.text('Panes are in their own windows.');
  await t.click('exact=Orchestration');
  await t.expect.noText('Panes are in their own windows.');
}

export async function run(t) {
  await calm(t);
  try {
    await body(t);
  } finally {
    await restoreOrchestration(t); // the mode is a saved setting: leave it as it was
  }
}
