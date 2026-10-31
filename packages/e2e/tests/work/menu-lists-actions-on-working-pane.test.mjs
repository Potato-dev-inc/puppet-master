// The one right-click menu on a pane that is working: every action is offered, Escape closes it, and the agent is not disturbed.
import { calm, S, oneTurn, openPane, waitStatus } from './_area.mjs';

export const meta = {
  id: 'work.menu-lists-actions-on-working-pane',
  title: 'Right-clicking a working pane lists Rename, Reflow, Copy selection, Interrupt, Details and Close; Escape closes it',
  area: 'work',
  tags: ['work', 'menu'],
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  t.fake.script(oneTurn([S.think(20000), S.say('slow job done')]));
  const pane = await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await pane.write('slow job');
  await waitStatus(t, pane, 'running');

  await t.step('Right-click the row');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.waitFor('aria=Pane actions');
  for (const item of ['Rename', 'Reflow', 'Copy selection', 'Interrupt', 'Details', 'Close']) await t.expect.visible(`exact=${item}`);

  await t.step('Escape closes the menu and the agent keeps working');
  await t.press('Escape');
  await t.waitGone('aria=Pane actions');
  t.expect.equal((await pane.info()).status, 'running');

  await t.step('Details on a working pane shows the sheet and closes again');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('exact=Details');
  await t.expect.eventually(async () => /Details|Folder|Agent/.test(await t.page.text()), { message: 'the details sheet' });
  await t.press('Escape');
  t.expect.equal((await pane.info()).status, 'running');
}
