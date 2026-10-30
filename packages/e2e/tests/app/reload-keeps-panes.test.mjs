// Edge case: the cockpit window is reloaded (a crash of the page, or a developer reload). The agents run in the app, not in the
// window, so they are all still there with their names, their state and what their terminals showed.
import { calm, openPane, focusTerminal, sidebarTitles, PANE_TITLE } from './_area.mjs';
import { replyScript } from '../_helpers.mjs';

export const meta = {
  id: 'app.reload-keeps-panes',
  title: 'Reloading the cockpit window keeps every pane and what its terminal showed',
  area: 'app',
  tags: ['persistence'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  t.fake.script(replyScript('kept across the reload', 200));
  const first = await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  const second = await t.spawnPane('nimbus', { cwd: t.project('beta') });
  await openPane(t, 'Nimbus 1');
  await focusTerminal(t);
  await t.type('remember this');
  await t.press('Enter');
  await t.expect.text('reply: kept across the reload');

  await t.step('Give the second pane a name of its own');
  await openPane(t, 'Nimbus 2');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('exact=Rename');
  await t.page.fill('aria=Pane name', 'Beta work');
  await t.press('Enter');
  await t.expect.eventually(async () => (await sidebarTitles(t)).includes('Beta work'), { message: 'the second pane is named' });

  await t.step('Reload the window');
  await t.page.reload();
  await t.expect.text('Puppet Master');

  await t.step('Both panes are listed again, with their names');
  await t.expect.eventually(async () => (await sidebarTitles(t)).length === 2, { message: 'two rows', timeout: 15000 });
  const titles = await sidebarTitles(t);
  t.expect.includes(titles, 'Beta work');
  t.expect.equal((await t.panes()).map((p) => p.id).sort(), [first.id, second.id].sort(), 'the app still runs the same two agents');

  await t.step('The first terminal still shows the conversation');
  await t.click('exact=remember this', { within: 'css=.ck-side' }).catch(async () => openPane(t, titles.find((x) => x !== 'Beta work')));
  await t.waitFor(PANE_TITLE);
  await t.expect.text('reply: kept across the reload');
  await t.expect.text('Idle');
}
