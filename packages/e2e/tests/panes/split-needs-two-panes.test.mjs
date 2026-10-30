// Edge case: Ctrl+\ with nothing open, or with only one pane, says what is missing instead of doing nothing.
import { calm, openPane, toasts } from './_area.mjs';

export const meta = {
  id: 'panes.split-needs-two-panes',
  title: 'Ctrl+\\ without a second pane explains why there is no split',
  area: 'panes',
  tags: ['split', 'keyboard'],
  edge: true,
};

export async function run(t) {
  await calm(t);

  await t.step('Nothing is open: Ctrl+\\ asks for an agent');
  await t.press('Ctrl+\\');
  await t.expect.eventually(async () => (await toasts(t)).some((x) => x.includes('Pick an agent first to split the view')), { message: 'a notice asks to pick an agent' });
  await t.expect.notVisible('css=.ck-split');

  await t.step('One pane is open: Ctrl+\\ asks for a second agent');
  await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await t.press('Ctrl+\\');
  await t.expect.eventually(async () => (await toasts(t)).some((x) => x.includes('Open a second agent to split the view')), { message: 'a notice asks for a second agent' });
  await t.expect.notVisible('css=.ck-split');
  await t.expect.text('NIMBUS 0.4.2');
}
