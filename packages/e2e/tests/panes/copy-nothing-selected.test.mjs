// Edge case: Copy selection with nothing selected says so (and does not put an empty text on the clipboard).
import { calm, openPane, toasts } from './_area.mjs';

export const meta = {
  id: 'panes.copy-nothing-selected',
  title: 'Copy selection with nothing selected says "Nothing selected"',
  area: 'panes',
  tags: ['terminal'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await t.expect.text('NIMBUS 0.4.2');

  await t.step('Pane menu > Copy selection');
  await t.click('aria=Pane actions');
  await t.click('exact=Copy selection');

  await t.step('A notice says nothing was selected');
  await t.expect.eventually(async () => (await toasts(t)).some((x) => x.includes('Nothing selected')), { message: 'the notice "Nothing selected" shows' });
  await t.expect.notVisible('exact=Copied');
}
